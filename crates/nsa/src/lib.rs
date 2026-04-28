//! NASA-Sloan Atlas (NSA) galaxy catalog loader and downloader.
//!
//! The NSA is the canonical SDSS-derived extragalactic catalog: ~640,000
//! galaxies with positions, redshifts, Sérsic structural fits, and per-band
//! integrated photometry across the SDSS+GALEX bands (FUV, NUV, u, g, r, i, z).
//!
//! This crate exposes a curated subset of the columns relevant for galaxy
//! rendering pipelines. The full NSA `nsa_v1_0_1.fits` file has ~150 columns;
//! the unexposed ones (image flags, Petrosian fluxes, K-corrections, PCA
//! stellar-population fits, etc.) can be added if a downstream needs them.
//!
//! Source: https://www.sdss.org/dr17/manga/manga-target-selection/nsa/
//!
//! # Example
//!
//! ```no_run
//! use starfield::catalogs::StarCatalog;
//! use starfield_nsa::{download_nsa, NsaCatalog};
//! let path = download_nsa()?;
//! let cat = NsaCatalog::from_fits_file(&path)?;
//! println!("{} galaxies", cat.len());
//! # Ok::<(), starfield::StarfieldError>(())
//! ```

pub mod catalog;
pub mod downloader;

pub use catalog::{NsaCatalog, NsaEntry};
pub use downloader::download_nsa;
