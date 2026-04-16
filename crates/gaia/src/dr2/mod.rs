//! Gaia DR2 (Data Release 2) loader, downloader, and entry type.

pub mod catalog;
pub mod entry;
pub mod schema;

pub use catalog::{download_all, download_file, list_cached, Dr2Catalog};
pub use entry::{AstroParams, AstrometricExtra, BpRpPhotometry, Dr2Entry, RadialVelocity};
pub use schema::Dr2;
