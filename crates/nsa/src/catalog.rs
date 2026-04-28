//! NSA `NsaEntry` type and FITS BinTable loader.
//!
//! v1 surface area: a curated subset of NSA columns (position, redshift,
//! Sérsic structural fit, per-band fluxes + ivars). Adding more columns is
//! a matter of (1) extending [`NsaEntry`], (2) adding another column read
//! in [`NsaCatalog::from_fits_file`], (3) wiring the per-row materializer.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use nalgebra as na;
use serde::{Deserialize, Serialize};
use starfield::catalogs::{StarCatalog, StarData};
use starfield::{Result, StarfieldError};

use fitsio_pure::bintable::{
    parse_binary_table_columns, read_binary_column, BinaryColumnData, BinaryColumnDescriptor,
};
use fitsio_pure::hdu::{parse_fits, Hdu, HduInfo};

/// SDSS+GALEX broad-band order used in NSA's `SERSIC_FLUX` / `SERSIC_FLUX_IVAR`
/// arrays. Column index `i` in either array corresponds to `BANDS[i]`.
pub const BANDS: [&str; 7] = ["FUV", "NUV", "u", "g", "r", "i", "z"];

/// One galaxy from the NASA-Sloan Atlas.
///
/// All units follow NSA's published conventions (mostly arcsec, deg,
/// nanomaggies). See <https://www.sdss.org/dr17/manga/manga-target-selection/nsa/>
/// for the full column reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsaEntry {
    /// NSA's stable per-galaxy ID (`NSAID` column).
    pub nsaid: u32,
    /// J2000 right ascension (degrees).
    pub ra: f64,
    /// J2000 declination (degrees).
    pub dec: f64,
    /// Heliocentric redshift.
    pub z: f32,
    /// Sérsic effective (half-light) radius, arcsec, fit in r-band.
    pub sersic_th50: f32,
    /// Sérsic index *n*.
    pub sersic_n: f32,
    /// Sérsic axis ratio *b/a*.
    pub sersic_ba: f32,
    /// Sérsic position angle, degrees east of north.
    pub sersic_phi: f32,
    /// Total Sérsic flux per band (FUV, NUV, u, g, r, i, z), nanomaggies.
    pub sersic_flux: [f32; 7],
    /// Inverse variance of the Sérsic flux per band.
    pub sersic_flux_ivar: [f32; 7],
}

impl NsaEntry {
    /// Convert J2000 RA/Dec to a unit vector in ICRS Cartesian coordinates.
    pub fn unit_vector(&self) -> na::Vector3<f64> {
        let ra = self.ra.to_radians();
        let dec = self.dec.to_radians();
        na::Vector3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin())
    }

    /// Approximate AB magnitude for one band from the Sérsic flux. Returns
    /// `None` if the flux is non-positive (NSA stores zero/negative for
    /// unmeasured or pathological cases).
    pub fn ab_magnitude(&self, band_idx: usize) -> Option<f64> {
        let f = *self.sersic_flux.get(band_idx)?;
        if f <= 0.0 {
            return None;
        }
        Some(22.5 - 2.5 * (f as f64).log10())
    }
}

/// In-memory NSA catalog keyed on `NSAID`.
#[derive(Debug, Clone)]
pub struct NsaCatalog {
    entries: HashMap<u32, NsaEntry>,
}

impl NsaCatalog {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Load every galaxy from a NSA `.fits` file. Reads the file into memory,
    /// finds the first BinTable extension, and materializes the curated set
    /// of columns into [`NsaEntry`]s.
    ///
    /// Memory: ~3 GB peak for the raw file bytes plus ~80 MB for the typed
    /// entries (~120 B each × 640 k galaxies).
    pub fn from_fits_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(StarfieldError::IoError)?;

        let fits = parse_fits(&bytes).map_err(|e| {
            StarfieldError::DataError(format!("parse_fits({}): {}", path.display(), e))
        })?;

        let (hdu, tfields) = first_bintable(&fits.hdus).ok_or_else(|| {
            StarfieldError::DataError(format!("no BinTable extension found in {}", path.display()))
        })?;

        let columns = parse_binary_table_columns(&hdu.cards, tfields)
            .map_err(|e| StarfieldError::DataError(format!("parse_binary_table_columns: {}", e)))?;

        let by_name: HashMap<&str, usize> = columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.name.as_deref().map(|n| (n, i)))
            .collect();

        let nsaid = read_u32_col(&bytes, hdu, &columns, &by_name, "NSAID")?;
        let ra = read_f64_col(&bytes, hdu, &columns, &by_name, "RA")?;
        let dec = read_f64_col(&bytes, hdu, &columns, &by_name, "DEC")?;
        let z = read_f32_col(&bytes, hdu, &columns, &by_name, "Z")?;
        let th50 = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_TH50")?;
        let nser = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_N")?;
        let ba = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_BA")?;
        let phi = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_PHI")?;
        let flux = read_f32_array_col(&bytes, hdu, &columns, &by_name, "SERSIC_FLUX", 7)?;
        let flux_ivar = read_f32_array_col(&bytes, hdu, &columns, &by_name, "SERSIC_FLUX_IVAR", 7)?;

        let n = nsaid.len();
        for (label, len) in [
            ("RA", ra.len()),
            ("DEC", dec.len()),
            ("Z", z.len()),
            ("SERSIC_TH50", th50.len()),
            ("SERSIC_N", nser.len()),
            ("SERSIC_BA", ba.len()),
            ("SERSIC_PHI", phi.len()),
            ("SERSIC_FLUX", flux.len()),
            ("SERSIC_FLUX_IVAR", flux_ivar.len()),
        ] {
            if len != n {
                return Err(StarfieldError::DataError(format!(
                    "NSA column {} has {} rows but NSAID has {}",
                    label, len, n
                )));
            }
        }

        let mut entries = HashMap::with_capacity(n);
        for i in 0..n {
            let entry = NsaEntry {
                nsaid: nsaid[i],
                ra: ra[i],
                dec: dec[i],
                z: z[i],
                sersic_th50: th50[i],
                sersic_n: nser[i],
                sersic_ba: ba[i],
                sersic_phi: phi[i],
                sersic_flux: flux[i],
                sersic_flux_ivar: flux_ivar[i],
            };
            entries.insert(entry.nsaid, entry);
        }

        Ok(Self { entries })
    }

    pub fn insert(&mut self, e: NsaEntry) {
        self.entries.insert(e.nsaid, e);
    }
}

impl Default for NsaCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl StarCatalog for NsaCatalog {
    type Star = NsaEntry;

    fn get_star(&self, id: usize) -> Option<&Self::Star> {
        self.entries.get(&(id as u32))
    }
    fn stars(&self) -> impl Iterator<Item = &Self::Star> {
        self.entries.values()
    }
    fn len(&self) -> usize {
        self.entries.len()
    }
    fn filter<F>(&self, pred: F) -> Vec<&Self::Star>
    where
        F: Fn(&Self::Star) -> bool,
    {
        self.entries.values().filter(|e| pred(e)).collect()
    }
    fn star_data(&self) -> impl Iterator<Item = StarData> + '_ {
        // r-band Sérsic mag stands in for the magnitude scalar; g-r color
        // stands in for the b-v slot. Galaxies aren't stars, but downstream
        // tooling that operates on `StarData` (cone-search, mag filter)
        // still works for first-pass spatial / brightness queries.
        self.entries.values().map(|e| {
            let mag = e.ab_magnitude(4).unwrap_or(f64::INFINITY);
            let g_r = match (e.ab_magnitude(3), e.ab_magnitude(4)) {
                (Some(g), Some(r)) => Some(g - r),
                _ => None,
            };
            StarData::new(e.nsaid as u64, e.ra, e.dec, mag, g_r)
        })
    }
    fn filter_star_data<F>(&self, pred: F) -> Vec<StarData>
    where
        F: Fn(&StarData) -> bool,
    {
        self.star_data().filter(|s| pred(s)).collect()
    }
    fn brighter_than(&self, magnitude: f64) -> Vec<StarData> {
        self.filter_star_data(|s| s.magnitude <= magnitude)
    }
    fn stars_in_field(&self, ra_deg: f64, dec_deg: f64, fov_deg: f64) -> Vec<StarData> {
        let center = unit_vec(ra_deg, dec_deg);
        let cos_fov = (fov_deg.to_radians() / 2.0).cos();
        self.filter_star_data(|s| unit_vec(s.ra_deg(), s.dec_deg()).dot(&center) >= cos_fov)
    }
}

fn unit_vec(ra_deg: f64, dec_deg: f64) -> na::Vector3<f64> {
    let ra = ra_deg.to_radians();
    let dec = dec_deg.to_radians();
    na::Vector3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin())
}

// ---- column helpers --------------------------------------------------------

/// Return `(hdu, tfields)` for the first `BinaryTable` HDU in the file.
fn first_bintable(hdus: &[Hdu]) -> Option<(&Hdu, usize)> {
    hdus.iter().find_map(|h| match h.info {
        HduInfo::BinaryTable { tfields, .. } => Some((h, tfields)),
        _ => None,
    })
}

fn col_index(by_name: &HashMap<&str, usize>, name: &str) -> Result<usize> {
    by_name.get(name).copied().ok_or_else(|| {
        StarfieldError::DataError(format!("NSA: missing required column `{}`", name))
    })
}

fn read_col(
    bytes: &[u8],
    hdu: &Hdu,
    by_name: &HashMap<&str, usize>,
    name: &str,
) -> Result<BinaryColumnData> {
    let idx = col_index(by_name, name)?;
    read_binary_column(bytes, hdu, idx)
        .map_err(|e| StarfieldError::DataError(format!("read_binary_column({}): {}", name, e)))
}

fn read_u32_col(
    bytes: &[u8],
    hdu: &Hdu,
    _columns: &[BinaryColumnDescriptor],
    by_name: &HashMap<&str, usize>,
    name: &str,
) -> Result<Vec<u32>> {
    match read_col(bytes, hdu, by_name, name)? {
        BinaryColumnData::Int(v) => Ok(v.into_iter().map(|x| x as u32).collect()),
        BinaryColumnData::Long(v) => Ok(v.into_iter().map(|x| x as u32).collect()),
        BinaryColumnData::Short(v) => Ok(v.into_iter().map(|x| x as u32).collect()),
        BinaryColumnData::Byte(v) => Ok(v.into_iter().map(|x| x as u32).collect()),
        other => Err(StarfieldError::DataError(format!(
            "NSA column `{}` expected integer, got {:?}",
            name,
            std::mem::discriminant(&other)
        ))),
    }
}

fn read_f64_col(
    bytes: &[u8],
    hdu: &Hdu,
    _columns: &[BinaryColumnDescriptor],
    by_name: &HashMap<&str, usize>,
    name: &str,
) -> Result<Vec<f64>> {
    match read_col(bytes, hdu, by_name, name)? {
        BinaryColumnData::Double(v) => Ok(v),
        BinaryColumnData::Float(v) => Ok(v.into_iter().map(|x| x as f64).collect()),
        other => Err(StarfieldError::DataError(format!(
            "NSA column `{}` expected float, got {:?}",
            name,
            std::mem::discriminant(&other)
        ))),
    }
}

fn read_f32_col(
    bytes: &[u8],
    hdu: &Hdu,
    _columns: &[BinaryColumnDescriptor],
    by_name: &HashMap<&str, usize>,
    name: &str,
) -> Result<Vec<f32>> {
    match read_col(bytes, hdu, by_name, name)? {
        BinaryColumnData::Float(v) => Ok(v),
        BinaryColumnData::Double(v) => Ok(v.into_iter().map(|x| x as f32).collect()),
        other => Err(StarfieldError::DataError(format!(
            "NSA column `{}` expected float, got {:?}",
            name,
            std::mem::discriminant(&other)
        ))),
    }
}

fn read_f32_array_col(
    bytes: &[u8],
    hdu: &Hdu,
    columns: &[BinaryColumnDescriptor],
    by_name: &HashMap<&str, usize>,
    name: &str,
    expected_len: usize,
) -> Result<Vec<[f32; 7]>> {
    assert_eq!(
        expected_len, 7,
        "NSA per-band arrays are always 7-element (FUV/NUV/u/g/r/i/z)"
    );
    let idx = col_index(by_name, name)?;
    if columns[idx].repeat != expected_len {
        return Err(StarfieldError::DataError(format!(
            "NSA column `{}` has TFORM repeat {}, expected {}",
            name, columns[idx].repeat, expected_len
        )));
    }
    let flat: Vec<f32> = match read_binary_column(bytes, hdu, idx)
        .map_err(|e| StarfieldError::DataError(format!("read_binary_column({}): {}", name, e)))?
    {
        BinaryColumnData::Float(v) => v,
        BinaryColumnData::Double(v) => v.into_iter().map(|x| x as f32).collect(),
        other => {
            return Err(StarfieldError::DataError(format!(
                "NSA column `{}` expected float array, got {:?}",
                name,
                std::mem::discriminant(&other)
            )))
        }
    };
    if !flat.len().is_multiple_of(expected_len) {
        return Err(StarfieldError::DataError(format!(
            "NSA column `{}` flattened length {} is not a multiple of {}",
            name,
            flat.len(),
            expected_len
        )));
    }
    let n_rows = flat.len() / expected_len;
    let mut out = Vec::with_capacity(n_rows);
    for chunk in flat.chunks_exact(expected_len) {
        let mut a = [0f32; 7];
        a.copy_from_slice(chunk);
        out.push(a);
    }
    Ok(out)
}
