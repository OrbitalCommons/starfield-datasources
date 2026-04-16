//! DR2 catalog newtype.

use std::path::{Path, PathBuf};

use starfield::Result;

use crate::common::catalog::GaiaCatalogBase;
use crate::download::Downloader;
use crate::dr2::entry::Dr2Entry;
use crate::dr2::schema::Dr2;

#[derive(Debug)]
pub struct Dr2Catalog(pub GaiaCatalogBase<Dr2>);

impl Dr2Catalog {
    pub fn new() -> Self {
        Self(GaiaCatalogBase::new())
    }

    pub fn from_csv_file(path: impl AsRef<Path>, mag_limit: f64) -> Result<Self> {
        Ok(Self(GaiaCatalogBase::<Dr2>::from_csv_file(
            path, mag_limit,
        )?))
    }

    pub fn inner(&self) -> &GaiaCatalogBase<Dr2> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut GaiaCatalogBase<Dr2> {
        &mut self.0
    }

    pub fn into_inner(self) -> GaiaCatalogBase<Dr2> {
        self.0
    }

    pub fn insert(&mut self, entry: Dr2Entry) {
        self.0.insert(entry);
    }

    pub fn merge(&mut self, other: Self) {
        self.0.merge(other.0);
    }
}

impl Default for Dr2Catalog {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Dr2Catalog {
    type Target = GaiaCatalogBase<Dr2>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn download_file(filename: &str) -> Result<PathBuf> {
    Downloader::<Dr2>::download_file(filename)
}

pub fn download_all(max_files: Option<usize>) -> Result<Vec<PathBuf>> {
    Downloader::<Dr2>::download_all(max_files)
}

pub fn list_cached() -> Result<Vec<PathBuf>> {
    Downloader::<Dr2>::list_cached()
}
