//! Shared types and machinery used by every Gaia data release.

pub mod catalog;
pub mod core;
pub mod format;
pub mod parse;
pub mod reader;
pub mod traits;

pub use catalog::GaiaCatalogBase;
pub use core::{GaiaCore, VarFlag};
pub use reader::CsvSourceReader;
pub use traits::{GaiaRelease, GaiaSource, Release};
