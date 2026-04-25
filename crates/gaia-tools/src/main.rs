//! `gaia-excerpt` — apply a predicate to one or more Gaia CSV(.gz) files
//! (local paths or streamed from ESA's CDN) and write the kept entries to
//! sharded gzipped CSVs that round-trip through `Dr{1,2,3}Catalog::from_csv_file`.

fn main() {
    if let Err(e) = run() {
        eprintln!("gaia-excerpt: {}", e);
        std::process::exit(1);
    }
}

mod cli;
mod sort_step;

use cli::{Cli, ReleaseChoice, Sharder};
use indicatif::{ProgressBar, ProgressStyle};
use starfield::{Result, StarfieldError};
use starfield_gaia::download::Downloader;
use starfield_gaia::excerpt::{
    excerpt_csv_file_into, excerpt_csv_reader_into, HashIdShard, HealpixShard, IdRangeShard,
    ShardKey, ShardedCsvWriter,
};
use starfield_gaia::{Dr1, Dr2, Dr3, GaiaRelease, GaiaSource};

const MAX_CONNECT_RETRIES: u32 = 4; // total 5 attempts

fn run() -> Result<()> {
    use clap::Parser;
    let args = Cli::parse();
    args.validate()?;
    let release = args.effective_release()?;

    // Dispatch on release exactly once so the writer can live across every
    // input file in the run. (Previously the per-file `excerpt_csv_file`
    // convenience constructed a fresh writer each time, which `File::create`-
    // truncated every shard it touched and silently lost data.)
    match release {
        ReleaseChoice::Dr1 => run_release::<Dr1>(&args),
        ReleaseChoice::Dr2 => run_release::<Dr2>(&args),
        ReleaseChoice::Dr3 => run_release::<Dr3>(&args),
    }
}

fn run_release<R: GaiaRelease>(args: &Cli) -> Result<()> {
    let mag_limit = args.mag_limit.unwrap_or(f64::INFINITY);

    let sharder = make_sharder::<R::Entry>(args);
    let mut writer =
        ShardedCsvWriter::<R, Box<dyn ShardKey<R::Entry>>>::new(&args.output_dir, sharder)?;
    let mut predicate = build_predicate::<R::Entry>(args)?;

    let multi = indicatif::MultiProgress::new();
    let kept_pb = multi.add(ProgressBar::new_spinner());
    kept_pb.set_style(
        ProgressStyle::with_template("  kept: {pos} stars (across all shards)").unwrap(),
    );

    let mut input_rows_total: usize = 0;

    if !args.input.is_empty() {
        let file_pb = multi.add(ProgressBar::new(args.input.len() as u64));
        file_pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} input files ({eta})",
            )
            .unwrap(),
        );
        for input in &args.input {
            let rows =
                excerpt_csv_file_into::<R, _, _>(input, mag_limit, &mut writer, &mut predicate)?;
            input_rows_total += rows;
            if args.clean_after_excerpt {
                std::fs::remove_file(input).map_err(StarfieldError::IoError)?;
            }
            kept_pb.set_position(writer.kept_so_far());
            file_pb.inc(1);
        }
        file_pb.finish_with_message("done");
    } else {
        // CDN mode: enumerate, then per-file streamed-or-cached with retry.
        let names = Downloader::<R>::list_remote()?;
        let total_files = args.max_files.unwrap_or(names.len()).min(names.len());
        eprintln!(
            "fetching {} of {} {} files from CDN ({})",
            total_files,
            names.len(),
            release_label::<R>(),
            if args.cache_raw {
                "writing raw to cache"
            } else {
                "streaming, no raw on disk"
            },
        );
        let file_pb = multi.add(ProgressBar::new(total_files as u64));
        file_pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} CDN files ({eta})",
            )
            .unwrap(),
        );
        let mut skipped: Vec<(String, String)> = Vec::new();
        let mut partial: Vec<String> = Vec::new();

        for (idx, name) in names.iter().take(total_files).enumerate() {
            let outcome = retry_one_file::<R>(
                idx,
                total_files,
                name,
                args,
                mag_limit,
                &mut writer,
                &mut predicate,
                &mut input_rows_total,
            );
            match outcome {
                FileOutcome::Ok => {}
                FileOutcome::PartialMidStream { rows_written, err } => {
                    eprintln!(
                        "[{}/{}] {}: mid-stream failure after {} rows kept ({}); \
                         continuing with partial data from this file",
                        idx + 1,
                        total_files,
                        name,
                        rows_written,
                        err
                    );
                    partial.push(name.clone());
                }
                FileOutcome::GivingUp { attempts, err } => {
                    eprintln!(
                        "[{}/{}] {}: giving up after {} connect attempts ({})",
                        idx + 1,
                        total_files,
                        name,
                        attempts,
                        err
                    );
                    skipped.push((name.clone(), err));
                }
            }
            kept_pb.set_position(writer.kept_so_far());
            file_pb.inc(1);
        }
        file_pb.finish_with_message("done");

        if !partial.is_empty() {
            eprintln!(
                "\n{} files had mid-stream failures (some rows may be missing from shards):",
                partial.len()
            );
            for p in &partial {
                eprintln!("  - {}", p);
            }
        }
        if !skipped.is_empty() {
            eprintln!(
                "\n{} files never completed a single successful connect:",
                skipped.len()
            );
            for (p, err) in &skipped {
                eprintln!("  - {}  ({})", p, err);
            }
        }
    }
    kept_pb.finish_and_clear();

    let summary = writer.finish()?;
    let written: Vec<_> = summary.written_paths().cloned().collect();
    println!(
        "read {} stars; kept {} ({:.2}%); wrote {} shard files",
        input_rows_total,
        summary.kept_rows,
        if input_rows_total == 0 {
            0.0
        } else {
            100.0 * summary.kept_rows as f64 / input_rows_total as f64
        },
        written.len(),
    );

    if args.sort {
        eprintln!(
            "\nsorting {} shard files by {:?} ...",
            written.len(),
            args.sort_by
        );
        let sort_pb = ProgressBar::new(written.len() as u64);
        sort_pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.green/blue} {pos}/{len} sorted ({eta})",
            )
            .unwrap(),
        );
        for path in &written {
            sort_step::sort_shard::<R>(path, args.sort_by)?;
            sort_pb.inc(1);
        }
        sort_pb.finish_with_message("sorted");
    }

    Ok(())
}

fn release_label<R: GaiaRelease>() -> &'static str {
    match R::RELEASE {
        starfield_gaia::Release::Dr1 => "DR1",
        starfield_gaia::Release::Dr2 => "DR2",
        starfield_gaia::Release::Dr3 => "DR3",
    }
}

fn make_sharder<E: GaiaSource + 'static>(args: &Cli) -> Box<dyn ShardKey<E>> {
    match args.shard_by {
        Sharder::Hash => Box::new(HashIdShard {
            num_shards: args.shards,
        }),
        Sharder::IdRange => Box::new(IdRangeShard {
            num_shards: args.shards,
        }),
        Sharder::Healpix => Box::new(HealpixShard {
            num_shards: args.shards,
            level: args.healpix_level,
        }),
    }
}

/// Outcome of one (retried) CDN file. `Ok` = file fully processed. `PartialMidStream`
/// = the HTTP body errored mid-read after some rows had already been written to
/// shards; we don't retry (would duplicate those rows) and move on. `GivingUp` =
/// failed before a single row was kept and exhausted retries.
enum FileOutcome {
    Ok,
    PartialMidStream { rows_written: u64, err: String },
    GivingUp { attempts: u32, err: String },
}

#[allow(clippy::too_many_arguments)]
fn retry_one_file<R>(
    idx: usize,
    total: usize,
    name: &str,
    args: &Cli,
    mag_limit: f64,
    writer: &mut ShardedCsvWriter<R, Box<dyn ShardKey<R::Entry>>>,
    predicate: &mut BoxedPredicate<R::Entry>,
    input_rows_total: &mut usize,
) -> FileOutcome
where
    R: GaiaRelease,
{
    for attempt in 0..=MAX_CONNECT_RETRIES {
        let kept_before = writer.kept_so_far();
        let res: Result<usize> = if args.cache_raw {
            match Downloader::<R>::download_file(name) {
                Ok(path) => {
                    let extract =
                        excerpt_csv_file_into::<R, _, _>(&path, mag_limit, writer, &mut *predicate);
                    // On successful extract, optionally evict the cached raw
                    // file so disk usage stays bounded (one file in flight).
                    // Failure to delete is a warning, not an error — the
                    // rows are already committed to shards.
                    if extract.is_ok() && args.clean_after_excerpt {
                        if let Err(e) = std::fs::remove_file(&path) {
                            eprintln!(
                                "[{}/{}] {}: warn: extract ok but failed to evict {}: {}",
                                idx + 1,
                                total,
                                name,
                                path.display(),
                                e
                            );
                        }
                    }
                    extract
                }
                Err(e) => Err(e),
            }
        } else {
            match Downloader::<R>::stream_file(name) {
                Ok(stream) => excerpt_csv_reader_into::<R, _, _>(
                    stream,
                    true,
                    mag_limit,
                    writer,
                    &mut *predicate,
                ),
                Err(e) => Err(e),
            }
        };
        match res {
            Ok(rows) => {
                *input_rows_total += rows;
                return FileOutcome::Ok;
            }
            Err(e) => {
                let err_str = e.to_string();
                let rows_written = writer.kept_so_far().saturating_sub(kept_before);
                if rows_written > 0 {
                    return FileOutcome::PartialMidStream {
                        rows_written,
                        err: err_str,
                    };
                }
                if attempt == MAX_CONNECT_RETRIES {
                    return FileOutcome::GivingUp {
                        attempts: attempt + 1,
                        err: err_str,
                    };
                }
                let wait = 1u64 << attempt; // 1, 2, 4, 8, 16 seconds
                eprintln!(
                    "[{}/{}] {}: attempt {}/{} failed ({}); retrying in {}s",
                    idx + 1,
                    total,
                    name,
                    attempt + 1,
                    MAX_CONNECT_RETRIES + 1,
                    err_str,
                    wait,
                );
                std::thread::sleep(std::time::Duration::from_secs(wait));
            }
        }
    }
    FileOutcome::Ok
}

type BoxedPredicate<E> = Box<dyn FnMut(&E) -> bool + Send>;

/// Compose mag/cone/id-range filters into a single closure. Cone filtering
/// is done with great-circle distance via core unit vectors.
fn build_predicate<E: GaiaSource + 'static>(args: &Cli) -> Result<BoxedPredicate<E>> {
    let cone = args.cone;
    let id_range = args.id_range;
    let cone_threshold = cone.as_ref().map(|c| (c.radius_deg.to_radians()).cos());
    let cone_center = cone.as_ref().map(|c| {
        let ra_rad = c.ra_deg.to_radians();
        let dec_rad = c.dec_deg.to_radians();
        nalgebra::Vector3::new(
            dec_rad.cos() * ra_rad.cos(),
            dec_rad.cos() * ra_rad.sin(),
            dec_rad.sin(),
        )
    });

    Ok(Box::new(move |e: &E| {
        let c = e.core();
        if let Some((lo, hi)) = id_range {
            if c.source_id < lo || c.source_id > hi {
                return false;
            }
        }
        if let (Some(center), Some(threshold)) = (cone_center.as_ref(), cone_threshold) {
            let v = c.unit_vector();
            if v.dot(center) < threshold {
                return false;
            }
        }
        true
    }))
}
