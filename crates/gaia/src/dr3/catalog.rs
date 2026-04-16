//! DR3 catalog newtype — thin wrapper over [`GaiaCatalogBase`](crate::common::catalog::GaiaCatalogBase).

use std::path::{Path, PathBuf};

use starfield::Result;

use crate::common::catalog::GaiaCatalogBase;
use crate::download::Downloader;
use crate::dr3::entry::Dr3Entry;
use crate::dr3::schema::Dr3;

/// In-memory Gaia DR3 catalog, keyed by `source_id`.
#[derive(Debug)]
pub struct Dr3Catalog(pub GaiaCatalogBase<Dr3>);

impl Dr3Catalog {
    pub fn new() -> Self {
        Self(GaiaCatalogBase::new())
    }

    /// Load a single `.csv` or `.csv.gz` file; discards stars fainter than `mag_limit`.
    pub fn from_csv_file(path: impl AsRef<Path>, mag_limit: f64) -> Result<Self> {
        Ok(Self(GaiaCatalogBase::<Dr3>::from_csv_file(
            path, mag_limit,
        )?))
    }

    pub fn inner(&self) -> &GaiaCatalogBase<Dr3> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut GaiaCatalogBase<Dr3> {
        &mut self.0
    }

    pub fn into_inner(self) -> GaiaCatalogBase<Dr3> {
        self.0
    }

    pub fn insert(&mut self, entry: Dr3Entry) {
        self.0.insert(entry);
    }

    pub fn merge(&mut self, other: Self) {
        self.0.merge(other.0);
    }
}

impl Default for Dr3Catalog {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Dr3Catalog {
    type Target = GaiaCatalogBase<Dr3>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// -- Download conveniences --------------------------------------------------

/// Download a specific DR3 `GaiaSource_*.csv.gz` file from ESA's CDN.
pub fn download_file(filename: &str) -> Result<PathBuf> {
    Downloader::<Dr3>::download_file(filename)
}

/// Download every DR3 `GaiaSource_*.csv.gz` (optionally capped at `max_files`).
pub fn download_all(max_files: Option<usize>) -> Result<Vec<PathBuf>> {
    Downloader::<Dr3>::download_all(max_files)
}

/// Cached DR3 files on disk, if any.
pub fn list_cached() -> Result<Vec<PathBuf>> {
    Downloader::<Dr3>::list_cached()
}
