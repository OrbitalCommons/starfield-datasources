//! Astronomical data source clients for the starfield ecosystem.
//!
//! This crate re-exports all starfield datasource crates behind feature flags.
//! By default, all datasources are enabled.
//!
//! # Feature flags
//!
//! - `horizons` — NASA JPL HORIZONS API client
//! - `sbdb` — NASA JPL Small-Body Database API client
//! - `gaia` — ESA Gaia DR3 loader (default)
//! - `gaia-all` — adds DR1 and DR2 alongside DR3
//! - `hipparcos` — Hipparcos star catalog loader
//! - `mpc` — Minor Planet Center client (MPCORB, observatory codes, observations)
//! - `nsa` — NASA-Sloan Atlas (NSA) galaxy catalog loader
//! - `rubin` — Vera C. Rubin Observatory LSST alert broker clients

#[cfg(feature = "horizons")]
pub use starfield_horizons as horizons;

#[cfg(feature = "sbdb")]
pub use starfield_sbdb as sbdb;

#[cfg(feature = "gaia")]
pub use starfield_gaia as gaia;

#[cfg(feature = "hipparcos")]
pub use starfield_hipparcos as hipparcos;

#[cfg(feature = "mpc")]
pub use starfield_mpc as mpc;

#[cfg(feature = "nsa")]
pub use starfield_nsa as nsa;

#[cfg(feature = "rubin")]
pub use starfield_rubin as rubin;
