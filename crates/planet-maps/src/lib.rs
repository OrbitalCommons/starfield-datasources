//! Equirectangular planetary albedo maps in body-fixed coordinates.
//!
//! The spatial half of resolved-body appearance: where the light comes from on
//! the disk. Spectral albedos ([`starfield-planet-spectra`]) and endmember
//! reflectances ([`starfield-reflectance-library`]) say what colour it is.
//!
//! # Conventions are data, not assumptions
//!
//! The archives disagree about longitude handedness and latitude definition,
//! and a map registered with the wrong handedness is *mirrored* — which looks
//! entirely plausible until it is checked against an ephemeris. Every product
//! therefore carries a [`MapGrid`] describing how it is stored, and all
//! conversion happens inside this crate.
//!
//! Callers pass **east-positive planetocentric radians**, which is exactly what
//! `starfield`'s body-fixed frames and `SubPoint::to_planetocentric` produce.
//! Do not apply a `LongitudeSense` first; that is for matching Horizons and IAU
//! cartographic output, not for sampling a map.
//!
//! [`starfield-planet-spectra`]: https://docs.rs/starfield-planet-spectra
//! [`starfield-reflectance-library`]: https://docs.rs/starfield-reflectance-library

pub mod grid;

pub use grid::{
    planetocentric_to_planetographic, planetographic_to_planetocentric, Latitude, Longitude,
    MapGrid, RowOrder,
};
