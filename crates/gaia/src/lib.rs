//! ESA Gaia star catalog loader and downloader.

pub mod catalog;
pub mod downloader;

pub use catalog::{GaiaCatalog, GaiaEntry};
pub use downloader::{
    download_gaia_catalog, download_gaia_file, get_gaia_cache_dir, list_cached_gaia_files,
};
