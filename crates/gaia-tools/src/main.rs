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
    excerpt_csv_file, excerpt_csv_reader, ExcerptSummary, HashIdShard, HealpixShard, IdRangeShard,
};
use starfield_gaia::{Dr1, Dr2, Dr3, GaiaRelease, GaiaSource};
use std::path::{Path, PathBuf};

fn run() -> Result<()> {
    use clap::Parser;
    let args = Cli::parse();
    args.validate()?;
    let release = args.effective_release()?;

    let multi = indicatif::MultiProgress::new();
    let kept_pb = multi.add(ProgressBar::new_spinner());
    kept_pb.set_style(
        ProgressStyle::with_template("  kept: {pos} stars across {msg} shards").unwrap(),
    );

    let mut totals = Totals::default();

    if !args.input.is_empty() {
        let file_pb = multi.add(ProgressBar::new(args.input.len() as u64));
        file_pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} input files ({eta})",
            )
            .unwrap(),
        );
        for input in &args.input {
            match release {
                ReleaseChoice::Dr1 => run_local::<Dr1>(input, &args, &mut totals, &kept_pb)?,
                ReleaseChoice::Dr2 => run_local::<Dr2>(input, &args, &mut totals, &kept_pb)?,
                ReleaseChoice::Dr3 => run_local::<Dr3>(input, &args, &mut totals, &kept_pb)?,
            }
            if args.clean_after_excerpt {
                std::fs::remove_file(input).map_err(StarfieldError::IoError)?;
            }
            file_pb.inc(1);
        }
        file_pb.finish_with_message("done");
    } else {
        // CDN mode: enumerate, then per-file stream-or-cache.
        let names = match release {
            ReleaseChoice::Dr1 => Downloader::<Dr1>::list_remote()?,
            ReleaseChoice::Dr2 => Downloader::<Dr2>::list_remote()?,
            ReleaseChoice::Dr3 => Downloader::<Dr3>::list_remote()?,
        };
        let total_files = args.max_files.unwrap_or(names.len()).min(names.len());
        eprintln!(
            "fetching {} of {} {} files from CDN ({})",
            total_files,
            names.len(),
            release.as_str(),
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
        for name in names.iter().take(total_files) {
            match release {
                ReleaseChoice::Dr1 => run_cdn::<Dr1>(name, &args, &mut totals, &kept_pb)?,
                ReleaseChoice::Dr2 => run_cdn::<Dr2>(name, &args, &mut totals, &kept_pb)?,
                ReleaseChoice::Dr3 => run_cdn::<Dr3>(name, &args, &mut totals, &kept_pb)?,
            }
            file_pb.inc(1);
        }
        file_pb.finish_with_message("done");
    }
    kept_pb.finish_and_clear();

    let inputs_processed = if args.input.is_empty() {
        args.max_files.unwrap_or(0)
    } else {
        args.input.len()
    };
    println!(
        "read {} stars across {} files; kept {} ({:.2}%); wrote {} shard files",
        totals.input_rows,
        inputs_processed,
        totals.kept_rows,
        if totals.input_rows == 0 {
            0.0
        } else {
            100.0 * totals.kept_rows as f64 / totals.input_rows as f64
        },
        totals.distinct_shard_files.len(),
    );

    if args.sort {
        eprintln!(
            "\nsorting {} shard files by {:?} ...",
            totals.distinct_shard_files.len(),
            args.sort_by
        );
        let sort_pb = ProgressBar::new(totals.distinct_shard_files.len() as u64);
        sort_pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.green/blue} {pos}/{len} sorted ({eta})",
            )
            .unwrap(),
        );
        for path in &totals.distinct_shard_files {
            match release {
                ReleaseChoice::Dr1 => sort_step::sort_shard::<Dr1>(path, args.sort_by)?,
                ReleaseChoice::Dr2 => sort_step::sort_shard::<Dr2>(path, args.sort_by)?,
                ReleaseChoice::Dr3 => sort_step::sort_shard::<Dr3>(path, args.sort_by)?,
            }
            sort_pb.inc(1);
        }
        sort_pb.finish_with_message("sorted");
    }

    Ok(())
}

#[derive(Default)]
struct Totals {
    input_rows: usize,
    kept_rows: u64,
    distinct_shard_files: Vec<PathBuf>,
}

fn run_local<R>(input: &Path, args: &Cli, totals: &mut Totals, kept_pb: &ProgressBar) -> Result<()>
where
    R: GaiaRelease,
{
    let predicate = build_predicate::<R::Entry>(args)?;
    let summary = match args.shard_by {
        Sharder::Hash => excerpt_csv_file::<R, _, _>(
            input,
            args.mag_limit.unwrap_or(f64::INFINITY),
            &args.output_dir,
            HashIdShard {
                num_shards: args.shards,
            },
            predicate,
        )?,
        Sharder::IdRange => excerpt_csv_file::<R, _, _>(
            input,
            args.mag_limit.unwrap_or(f64::INFINITY),
            &args.output_dir,
            IdRangeShard {
                num_shards: args.shards,
            },
            predicate,
        )?,
        Sharder::Healpix => excerpt_csv_file::<R, _, _>(
            input,
            args.mag_limit.unwrap_or(f64::INFINITY),
            &args.output_dir,
            HealpixShard {
                num_shards: args.shards,
                level: args.healpix_level,
            },
            predicate,
        )?,
    };
    accumulate(totals, summary, kept_pb);
    Ok(())
}

fn run_cdn<R>(filename: &str, args: &Cli, totals: &mut Totals, kept_pb: &ProgressBar) -> Result<()>
where
    R: GaiaRelease,
{
    let predicate = build_predicate::<R::Entry>(args)?;

    // Pick the byte source: cached or streamed.
    let summary = if args.cache_raw {
        let path = Downloader::<R>::download_file(filename)?;
        match args.shard_by {
            Sharder::Hash => excerpt_csv_file::<R, _, _>(
                &path,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                HashIdShard {
                    num_shards: args.shards,
                },
                predicate,
            )?,
            Sharder::IdRange => excerpt_csv_file::<R, _, _>(
                &path,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                IdRangeShard {
                    num_shards: args.shards,
                },
                predicate,
            )?,
            Sharder::Healpix => excerpt_csv_file::<R, _, _>(
                &path,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                HealpixShard {
                    num_shards: args.shards,
                    level: args.healpix_level,
                },
                predicate,
            )?,
        }
    } else {
        let stream = Downloader::<R>::stream_file(filename)?;
        // Filenames come from the index regex which targets `*.csv.gz` only,
        // so is_gz is always true for CDN-sourced files.
        let is_gz = filename.ends_with(".gz");
        match args.shard_by {
            Sharder::Hash => excerpt_csv_reader::<R, _, _>(
                stream,
                is_gz,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                HashIdShard {
                    num_shards: args.shards,
                },
                predicate,
            )?,
            Sharder::IdRange => excerpt_csv_reader::<R, _, _>(
                stream,
                is_gz,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                IdRangeShard {
                    num_shards: args.shards,
                },
                predicate,
            )?,
            Sharder::Healpix => excerpt_csv_reader::<R, _, _>(
                stream,
                is_gz,
                args.mag_limit.unwrap_or(f64::INFINITY),
                &args.output_dir,
                HealpixShard {
                    num_shards: args.shards,
                    level: args.healpix_level,
                },
                predicate,
            )?,
        }
    };
    accumulate(totals, summary, kept_pb);
    Ok(())
}

fn accumulate(totals: &mut Totals, s: ExcerptSummary, kept_pb: &ProgressBar) {
    totals.input_rows += s.input_rows;
    totals.kept_rows += s.kept_rows;
    for p in s.written_paths() {
        if !totals.distinct_shard_files.iter().any(|x| x == p) {
            totals.distinct_shard_files.push(p.clone());
        }
    }
    kept_pb.set_position(totals.kept_rows);
    kept_pb.set_message(totals.distinct_shard_files.len().to_string());
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
