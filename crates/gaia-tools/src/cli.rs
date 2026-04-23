//! `clap`-based CLI surface for `gaia-excerpt`.

use clap::{Parser, ValueEnum};
use starfield::{Result, StarfieldError};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "gaia-excerpt",
    version,
    about = "Subset and shard Gaia catalog files"
)]
pub struct Cli {
    /// Input `.csv` or `.csv.gz` files (one or more local paths). Mutually
    /// exclusive with `--from-release`.
    #[arg(long = "input", num_args = 1.., conflicts_with = "from_release")]
    pub input: Vec<PathBuf>,

    /// Pull the inputs straight from ESA's CDN for the given release. Combined
    /// with `--max-files N` to limit how many files are processed; default is
    /// every file in the release. By default the raw bytes are streamed and
    /// never written to disk; pass `--cache-raw` to keep them under the
    /// per-release cache (`~/.cache/starfield/gaia/dr*/`).
    #[arg(long = "from-release", value_enum, conflicts_with = "input")]
    pub from_release: Option<ReleaseChoice>,

    /// Maximum number of CDN files to process when `--from-release` is set.
    #[arg(long = "max-files")]
    pub max_files: Option<usize>,

    /// When `--from-release` is set, write each downloaded raw file to the
    /// per-release cache. Default streams the bytes through and discards the
    /// raw to keep disk usage low.
    #[arg(long = "cache-raw", default_value_t = false)]
    pub cache_raw: bool,

    /// Delete each `--input` file from disk after successful processing. Local
    /// paths only; no-op on streamed CDN sources.
    #[arg(long = "clean-after-excerpt", default_value_t = false)]
    pub clean_after_excerpt: bool,

    /// Output directory for shard files. Created if missing.
    #[arg(long = "output-dir", short = 'o')]
    pub output_dir: PathBuf,

    /// Which Gaia data release the inputs come from. Required when `--input`
    /// is used; ignored when `--from-release` is used (the value is taken
    /// from the `--from-release` flag).
    #[arg(long = "release", value_enum)]
    pub release: Option<ReleaseChoice>,

    /// Sharding strategy.
    #[arg(long = "shard-by", value_enum, default_value_t = Sharder::Hash)]
    pub shard_by: Sharder,

    /// Number of shard files to write into.
    #[arg(long = "shards", default_value_t = 64)]
    pub shards: u32,

    /// HEALPix depth used by `--shard-by healpix` (0..=29).
    #[arg(long = "healpix-level", default_value_t = 6)]
    pub healpix_level: u8,

    /// Discard rows with `phot_g_mean_mag` greater than this at read time.
    #[arg(long = "mag-limit")]
    pub mag_limit: Option<f64>,

    /// Cone filter "RA,DEC,RADIUS_DEG" (degrees, ICRS). All combinable filters
    /// are AND'd together with `--mag-limit` and `--id-range`.
    #[arg(long = "cone", value_parser = parse_cone)]
    pub cone: Option<Cone>,

    /// Source-id range filter "LOW,HIGH" (inclusive both ends).
    #[arg(long = "id-range", value_parser = parse_id_range)]
    pub id_range: Option<(u64, u64)>,

    /// After every input is processed, sort each shard file by `--sort-by`.
    /// Sort is in-memory per shard (so each shard must fit in RAM).
    #[arg(long = "sort", default_value_t = false)]
    pub sort: bool,

    /// Field to sort each shard by when `--sort` is set.
    #[arg(long = "sort-by", value_enum, default_value_t = SortField::SourceId)]
    pub sort_by: SortField,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChoice {
    Dr1,
    Dr2,
    Dr3,
}

impl ReleaseChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReleaseChoice::Dr1 => "DR1",
            ReleaseChoice::Dr2 => "DR2",
            ReleaseChoice::Dr3 => "DR3",
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharder {
    Hash,
    IdRange,
    Healpix,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    SourceId,
    PhotGMeanMag,
    Ra,
    RandomIndex,
}

#[derive(Debug, Clone, Copy)]
pub struct Cone {
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub radius_deg: f64,
}

fn parse_cone(s: &str) -> Result<Cone> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err(StarfieldError::DataError(format!(
            "--cone wants RA,DEC,RADIUS_DEG; got {:?}",
            s
        )));
    }
    let ra_deg = parts[0]
        .parse::<f64>()
        .map_err(|e| StarfieldError::DataError(format!("--cone ra: {}", e)))?;
    let dec_deg = parts[1]
        .parse::<f64>()
        .map_err(|e| StarfieldError::DataError(format!("--cone dec: {}", e)))?;
    let radius_deg = parts[2]
        .parse::<f64>()
        .map_err(|e| StarfieldError::DataError(format!("--cone radius: {}", e)))?;
    Ok(Cone {
        ra_deg,
        dec_deg,
        radius_deg,
    })
}

impl Cli {
    /// Resolve the effective release: `--from-release` if set, otherwise
    /// `--release`, otherwise an error.
    pub fn effective_release(&self) -> Result<ReleaseChoice> {
        match (self.from_release, self.release) {
            (Some(r), _) => Ok(r),
            (None, Some(r)) => Ok(r),
            (None, None) => Err(StarfieldError::DataError(
                "must pass either --release (with --input) or --from-release (with CDN mode)"
                    .into(),
            )),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.input.is_empty() && self.from_release.is_none() {
            return Err(StarfieldError::DataError(
                "must pass either --input <files> or --from-release <release>".into(),
            ));
        }
        Ok(())
    }
}

fn parse_id_range(s: &str) -> Result<(u64, u64)> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 2 {
        return Err(StarfieldError::DataError(format!(
            "--id-range wants LOW,HIGH; got {:?}",
            s
        )));
    }
    let lo = parts[0]
        .parse::<u64>()
        .map_err(|e| StarfieldError::DataError(format!("--id-range low: {}", e)))?;
    let hi = parts[1]
        .parse::<u64>()
        .map_err(|e| StarfieldError::DataError(format!("--id-range high: {}", e)))?;
    if lo > hi {
        return Err(StarfieldError::DataError(format!(
            "--id-range low ({}) must be <= high ({})",
            lo, hi
        )));
    }
    Ok((lo, hi))
}
