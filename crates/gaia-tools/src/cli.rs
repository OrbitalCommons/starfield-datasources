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
    /// Input `.csv` or `.csv.gz` files (one or more).
    #[arg(long = "input", required = true, num_args = 1..)]
    pub input: Vec<PathBuf>,

    /// Output directory for shard files. Created if missing.
    #[arg(long = "output-dir", short = 'o')]
    pub output_dir: PathBuf,

    /// Which Gaia data release the inputs come from.
    #[arg(long = "release", value_enum)]
    pub release: ReleaseChoice,

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
