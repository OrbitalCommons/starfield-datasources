//! Astronomical data source clients for the starfield ecosystem.
//!
//! This crate re-exports all starfield datasource crates behind feature flags.
//! By default, all datasources are enabled.
//!
//! # Feature flags
//!
//! - `jplephem` — JPL Development Ephemeris reader (SPK/DAF)
//! - `horizons` — NASA JPL HORIZONS API client
//! - `sbdb` — NASA JPL Small-Body Database API client
//! - `gaia` — ESA Gaia star catalog loader
//! - `hipparcos` — Hipparcos star catalog loader

#[cfg(feature = "jplephem")]
pub use starfield_jplephem as jplephem;

#[cfg(feature = "horizons")]
pub use starfield_horizons as horizons;

#[cfg(feature = "sbdb")]
pub use starfield_sbdb as sbdb;

#[cfg(feature = "gaia")]
pub use starfield_gaia as gaia;

#[cfg(feature = "hipparcos")]
pub use starfield_hipparcos as hipparcos;
