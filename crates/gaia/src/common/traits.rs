//! Traits shared across every Gaia data release.

use crate::common::core::GaiaCore;
use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;
use starfield::Result;

/// Which Gaia data release an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Release {
    Dr1,
    Dr2,
    Dr3,
}

impl Release {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dr1 => "DR1",
            Self::Dr2 => "DR2",
            Self::Dr3 => "DR3",
        }
    }
}

/// Every per-release Entry type exposes its shared core and which release produced it.
pub trait GaiaSource {
    fn core(&self) -> &GaiaCore;
    fn release(&self) -> Release;

    /// Approximate B-V color index derived from Gaia photometry. DR2/DR3 have BP-RP,
    /// which correlates with but is not identical to B-V; releases that lack BP/RP
    /// return `None`. Used to populate `StarData::b_v` when Gaia catalogs are exposed
    /// through the generic [`StarCatalog`](starfield::catalogs::StarCatalog) trait.
    fn b_v(&self) -> Option<f64> {
        None
    }
}

/// Release-specific configuration for downloading, parsing, and caching.
///
/// Each of `Dr1`, `Dr2`, `Dr3` implements this trait so the generic reader/downloader
/// can be parameterized over the release without conditional logic at the call site.
pub trait GaiaRelease: 'static {
    const RELEASE: Release;

    /// HTTP base URL for the CSV.gz catalog files.
    const BASE_URL: &'static str;

    /// Filename within [`BASE_URL`] that holds the MD5 checksums for every file.
    const MD5_FILENAME: &'static str;

    /// Regex matching valid per-release catalog filenames on the index page.
    const FILE_REGEX: &'static str;

    /// Subdirectory under the starfield cache root (e.g. `"gaia/dr3"`).
    const CACHE_SUBDIR: &'static str;

    /// True if the file uses ECSV (leading `#`-prefixed YAML header before CSV).
    const IS_ECSV: bool = false;

    /// The entry type produced by parsing one row.
    type Entry: GaiaSource + std::fmt::Debug + Send + 'static;

    /// Arrow schema describing the columns this release's parser consumes.
    fn arrow_schema() -> SchemaRef;

    /// Build one entry from row `row` of `batch`. Columns are in the order declared
    /// by [`arrow_schema`].
    fn build_entry(batch: &RecordBatch, row: usize) -> Result<Self::Entry>;

    /// Format an entry as one CSV row matching the column layout in [`arrow_schema`].
    /// Floats use Rust's default `Display` (full round-trip precision); `Option::None`
    /// becomes the empty string. Output is parseable by [`from_csv_file`](crate::common::catalog::GaiaCatalogBase::from_csv_file).
    fn format_csv_row(entry: &Self::Entry) -> String;

    /// Comma-joined header line listing every column in [`arrow_schema`] order.
    fn csv_header() -> String {
        Self::arrow_schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect::<Vec<_>>()
            .join(",")
    }
}
