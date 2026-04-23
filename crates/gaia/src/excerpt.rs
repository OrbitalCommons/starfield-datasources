//! Sharded, incremental excerpt writer.
//!
//! Reads any Gaia CSV(.gz) catalog, applies a caller-supplied predicate, and
//! writes the kept entries to one of N shard files chosen by a [`ShardKey`]. Each
//! shard is written as gzipped CSV in the source release's own column layout, so
//! the output round-trips back through `Dr{1,2,3}Catalog::from_csv_file`.
//!
//! Per-shard files are opened lazily (only when first written to) so excerpts
//! that touch only a few shards don't litter the output directory with empty
//! files.
//!
//! # Example
//!
//! ```no_run
//! use starfield_gaia::excerpt::{excerpt_csv_file, HashIdShard};
//! use starfield_gaia::Dr3;
//!
//! let summary = excerpt_csv_file::<Dr3, _, _>(
//!     "GaiaSource_000000-003111.csv.gz",
//!     20.0,
//!     "out/",
//!     HashIdShard { num_shards: 32 },
//!     |entry| entry.core.phot_g_mean_mag < 16.0,
//! )?;
//! println!("kept {} of {} stars across {} shards",
//!          summary.kept_rows, summary.input_rows, summary.shard_files.len());
//! # Ok::<(), starfield::StarfieldError>(())
//! ```

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;
use starfield::{Result, StarfieldError};

use crate::common::reader::CsvSourceReader;
use crate::common::traits::{GaiaRelease, GaiaSource};

// =====================================================================================
// Sharding trait + built-in embodiments
// =====================================================================================

/// Maps an entry to a shard index in `0..num_shards()`.
///
/// Built-in embodiments cover the common cases (hash-of-id, id-range banding,
/// HEALPix cell). Implement this trait directly to shard by anything else
/// (brightness, date, custom hash, etc.).
pub trait ShardKey<E> {
    /// Number of shards. Returned values from [`shard_of`](Self::shard_of) must lie in `0..num_shards()`.
    fn num_shards(&self) -> u32;

    /// The shard index for one entry. Must be `< num_shards()`.
    fn shard_of(&self, entry: &E) -> u32;
}

/// Shard by `hash(source_id) % num_shards`. Roughly even distribution; loses
/// any spatial / brightness locality. Use when you want shards of similar
/// size for parallel processing.
#[derive(Debug, Clone, Copy)]
pub struct HashIdShard {
    pub num_shards: u32,
}

impl<E: GaiaSource> ShardKey<E> for HashIdShard {
    fn num_shards(&self) -> u32 {
        self.num_shards
    }
    fn shard_of(&self, entry: &E) -> u32 {
        // SplitMix64 (see `mix_id`) is deterministic across runs, so the same
        // source_id always lands in the same shard — important for incremental
        // / resumable workflows. `std::hash`-based hashers are randomized per
        // process and would make this non-reproducible.
        let mixed = mix_id(entry.core().source_id);
        (mixed % self.num_shards as u64) as u32
    }
}

/// SplitMix64 — deterministic, very fast, good distribution. Used to break the
/// HEALPix-12 bit alignment in Gaia source_ids before bucketing.
fn mix_id(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Shard by source_id range — divide the `u64` ID space into `num_shards`
/// equal-width buckets. For DR2/DR3 source_ids encode HEALPix-12 in the high
/// bits, so this gives spatial locality "for free" (nearby pixels in adjacent
/// shards). For DR1 the encoding differs and locality is weaker.
#[derive(Debug, Clone, Copy)]
pub struct IdRangeShard {
    pub num_shards: u32,
}

impl<E: GaiaSource> ShardKey<E> for IdRangeShard {
    fn num_shards(&self) -> u32 {
        self.num_shards
    }
    fn shard_of(&self, entry: &E) -> u32 {
        let id = entry.core().source_id;
        let bucket_size = (u64::MAX / self.num_shards as u64).saturating_add(1);
        ((id / bucket_size) as u32).min(self.num_shards - 1)
    }
}

/// Shard by HEALPix cell at the given depth, modulo `num_shards`. Computes
/// the pixel from the entry's RA/Dec via [`cdshealpix::nested::hash`], so it
/// works identically across DR1/DR2/DR3.
#[derive(Debug, Clone, Copy)]
pub struct HealpixShard {
    pub num_shards: u32,
    /// HEALPix depth (0..=29). Higher = finer pixels. Default 6 (49,152 pixels).
    pub level: u8,
}

impl<E: GaiaSource> ShardKey<E> for HealpixShard {
    fn num_shards(&self) -> u32 {
        self.num_shards
    }
    fn shard_of(&self, entry: &E) -> u32 {
        let c = entry.core();
        let pixel = cdshealpix::nested::hash(self.level, c.ra.to_radians(), c.dec.to_radians());
        (pixel % self.num_shards as u64) as u32
    }
}

// =====================================================================================
// Sharded gzipped CSV writer
// =====================================================================================

/// Streaming sharded CSV.gz writer. Files are opened lazily — a shard that
/// receives no entries leaves no file behind.
pub struct ShardedCsvWriter<R: GaiaRelease, S: ShardKey<R::Entry>> {
    output_dir: PathBuf,
    shard_key: S,
    writers: Vec<Option<BufWriter<GzEncoder<File>>>>,
    counts: Vec<u64>,
    paths: Vec<Option<PathBuf>>,
    width: usize,
    _marker: PhantomData<R>,
}

impl<R: GaiaRelease, S: ShardKey<R::Entry>> ShardedCsvWriter<R, S> {
    /// Create a new writer rooted at `output_dir`. Creates the directory if missing.
    pub fn new(output_dir: impl AsRef<Path>, shard_key: S) -> Result<Self> {
        let output_dir = output_dir.as_ref().to_path_buf();
        fs::create_dir_all(&output_dir).map_err(StarfieldError::IoError)?;
        let n = shard_key.num_shards();
        if n == 0 {
            return Err(StarfieldError::DataError(
                "ShardedCsvWriter: num_shards must be > 0".into(),
            ));
        }
        let width = digits_for(n);
        let mut writers = Vec::with_capacity(n as usize);
        let mut paths = Vec::with_capacity(n as usize);
        for _ in 0..n {
            writers.push(None);
            paths.push(None);
        }
        Ok(Self {
            output_dir,
            shard_key,
            writers,
            counts: vec![0; n as usize],
            paths,
            width,
            _marker: PhantomData,
        })
    }

    /// Write one entry to its shard file, opening the file (and writing the CSV
    /// header) the first time that shard is touched.
    pub fn write(&mut self, entry: &R::Entry) -> Result<()> {
        let shard = self.shard_key.shard_of(entry);
        if shard >= self.writers.len() as u32 {
            return Err(StarfieldError::DataError(format!(
                "ShardKey returned {} for num_shards={}",
                shard,
                self.writers.len()
            )));
        }
        let idx = shard as usize;

        if self.writers[idx].is_none() {
            let filename = format!("shard_{:0width$}.csv.gz", shard, width = self.width);
            let path = self.output_dir.join(&filename);
            let file = File::create(&path).map_err(StarfieldError::IoError)?;
            let gz = GzEncoder::new(file, Compression::default());
            let mut buf = BufWriter::new(gz);
            writeln!(buf, "{}", R::csv_header()).map_err(StarfieldError::IoError)?;
            self.writers[idx] = Some(buf);
            self.paths[idx] = Some(path);
        }

        let row = R::format_csv_row(entry);
        let writer = self.writers[idx].as_mut().unwrap();
        writeln!(writer, "{}", row).map_err(StarfieldError::IoError)?;
        self.counts[idx] += 1;
        Ok(())
    }

    /// Finalize: flush every open shard file and return a summary.
    pub fn finish(mut self) -> Result<ExcerptSummary> {
        for w in self.writers.iter_mut() {
            if let Some(buf) = w.take() {
                let gz = buf
                    .into_inner()
                    .map_err(|e| StarfieldError::IoError(e.into_error()))?;
                gz.finish().map_err(StarfieldError::IoError)?;
            }
        }
        Ok(ExcerptSummary {
            input_rows: 0, // populated by `excerpt_csv_file`
            kept_rows: self.counts.iter().sum(),
            per_shard_counts: self.counts,
            shard_files: self.paths,
        })
    }
}

fn digits_for(n: u32) -> usize {
    // Number of digits needed to represent `n - 1`, with a minimum of 4 so file
    // listings sort sensibly even for small shard counts.
    let max_idx = n.saturating_sub(1);
    let nd = if max_idx == 0 {
        1
    } else {
        (max_idx as f64).log10().floor() as usize + 1
    };
    nd.max(4)
}

// =====================================================================================
// Summary + convenience function
// =====================================================================================

/// What the excerpt run produced. `shard_files[i]` is `Some(path)` iff shard `i`
/// received at least one entry.
#[derive(Debug, Clone)]
pub struct ExcerptSummary {
    pub input_rows: usize,
    pub kept_rows: u64,
    pub per_shard_counts: Vec<u64>,
    pub shard_files: Vec<Option<PathBuf>>,
}

impl ExcerptSummary {
    /// Paths of every shard file that was actually written (Some entries only).
    pub fn written_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.shard_files.iter().flatten()
    }
}

/// Read a single CSV(.gz) file, apply `predicate` to each row's typed entry,
/// and stream the kept entries into shard files in `output_dir`.
///
/// `mag_limit` is applied at read time as a fast pre-filter on `phot_g_mean_mag`;
/// rows fainter than the limit never materialize. `predicate` runs after the
/// mag-limit pass and gets full access to the typed entry (including any nested
/// sub-structs your release exposes).
pub fn excerpt_csv_file<R, S, F>(
    input_path: impl AsRef<Path>,
    mag_limit: f64,
    output_dir: impl AsRef<Path>,
    sharder: S,
    predicate: F,
) -> Result<ExcerptSummary>
where
    R: GaiaRelease,
    S: ShardKey<R::Entry>,
    F: FnMut(&R::Entry) -> bool,
{
    let reader = CsvSourceReader::<R>::open(input_path, mag_limit)?;
    let mut writer = ShardedCsvWriter::<R, S>::new(output_dir, sharder)?;
    let input_rows = drive(reader, &mut writer, predicate)?;
    let mut summary = writer.finish()?;
    summary.input_rows = input_rows;
    Ok(summary)
}

/// Same as [`excerpt_csv_file`] but reads from any byte source — typically a
/// streamed HTTP response from
/// [`Downloader::stream_file`](crate::download::Downloader::stream_file), so the
/// raw catalog never has to land on disk.
pub fn excerpt_csv_reader<R, S, F>(
    reader: Box<dyn std::io::Read>,
    is_gz: bool,
    mag_limit: f64,
    output_dir: impl AsRef<Path>,
    sharder: S,
    predicate: F,
) -> Result<ExcerptSummary>
where
    R: GaiaRelease,
    S: ShardKey<R::Entry>,
    F: FnMut(&R::Entry) -> bool,
{
    let reader = CsvSourceReader::<R>::from_reader(reader, is_gz, mag_limit)?;
    let mut writer = ShardedCsvWriter::<R, S>::new(output_dir, sharder)?;
    let input_rows = drive(reader, &mut writer, predicate)?;
    let mut summary = writer.finish()?;
    summary.input_rows = input_rows;
    Ok(summary)
}

fn drive<R, S, F>(
    reader: CsvSourceReader<R>,
    writer: &mut ShardedCsvWriter<R, S>,
    mut predicate: F,
) -> Result<usize>
where
    R: GaiaRelease,
    S: ShardKey<R::Entry>,
    F: FnMut(&R::Entry) -> bool,
{
    let mut input_rows = 0usize;
    for entry in reader {
        let entry = entry?;
        input_rows += 1;
        if predicate(&entry) {
            writer.write(&entry)?;
        }
    }
    Ok(input_rows)
}
