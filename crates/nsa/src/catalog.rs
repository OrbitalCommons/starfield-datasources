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

/// Number of bands in `NsaEntry`'s flux arrays. Always 7; v0_1_2 (5-band)
/// files are padded with zeros in the FUV/NUV slots so the in-memory layout
/// stays uniform.
pub const N_BANDS: usize = 7;

/// SDSS+GALEX broad-band order in `NsaEntry::sersic_flux` / `_ivar`. Index `i`
/// in either array corresponds to `BANDS[i]`. v0_1_2 files don't carry GALEX
/// FUV/NUV photometry — those slots are zero (and `ab_magnitude(0)` /
/// `ab_magnitude(1)` will return `None` accordingly).
pub const BANDS: [&str; N_BANDS] = ["FUV", "NUV", "u", "g", "r", "i", "z"];

/// Which NSA release a catalog was loaded from. The two are detected at load
/// time from the `SERSIC_FLUX` column's TFORM repeat (5 → v0_1_2, 7 → v1_0_1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsaVersion {
    /// 5-band SDSS-DR8-era release (`u, g, r, i, z`). Currently the only NSA
    /// file available for free download (NYU mirror, ~0.5 GB).
    V0_1_2,
    /// 7-band release (`FUV, NUV, u, g, r, i, z`). SDSS DR17 used to host this
    /// at `data.sdss.org/sas/dr17/manga/atlas/v1_0_1/nsa_v1_0_1.fits`; that path
    /// 404s as of 2026-04-28. If the loader sees a 7-element flux array we
    /// still parse it correctly.
    V1_0_1,
}

impl NsaVersion {
    /// Index of the `r` band in `NsaEntry::sersic_flux` for this version.
    /// Always 4 in the 7-slot in-memory layout (v0_1_2's r band is shifted
    /// from index 2 in-file to index 4 in-memory by the loader).
    pub const R_BAND_IDX: usize = 4;

    /// Index of the `g` band in the 7-slot in-memory layout.
    pub const G_BAND_IDX: usize = 3;

    /// Number of bands actually populated in the source FITS file.
    pub fn n_source_bands(&self) -> usize {
        match self {
            NsaVersion::V0_1_2 => 5,
            NsaVersion::V1_0_1 => 7,
        }
    }

    fn from_repeat(repeat: usize) -> Result<Self> {
        match repeat {
            5 => Ok(NsaVersion::V0_1_2),
            7 => Ok(NsaVersion::V1_0_1),
            other => Err(StarfieldError::DataError(format!(
                "NSA: SERSIC_FLUX repeat={} is neither 5 (v0_1_2) nor 7 (v1_0_1)",
                other
            ))),
        }
    }
}

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
    version: NsaVersion,
}

impl NsaCatalog {
    /// Empty catalog tagged as 7-band; useful for tests that build one up by
    /// hand. `from_fits_file` overrides this with whatever the file declares.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            version: NsaVersion::V1_0_1,
        }
    }

    /// Which NSA release this catalog was loaded from.
    pub fn version(&self) -> NsaVersion {
        self.version
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

        // v0_1_2 names the column `SERSICFLUX` (no underscore); v1_0_1 uses
        // `SERSIC_FLUX`. Same story for the inverse-variance companion.
        let flux_name = pick_name(&by_name, &["SERSIC_FLUX", "SERSICFLUX"])?;
        let flux_ivar_name = pick_name(&by_name, &["SERSIC_FLUX_IVAR", "SERSICFLUX_IVAR"])?;

        // Detect version from the resolved flux column's TFORM repeat. v0_1_2
        // is 5-band (u/g/r/i/z); v1_0_1 is 7-band (adds GALEX FUV/NUV at
        // indices 0-1).
        let flux_idx = col_index(&by_name, flux_name)?;
        let version = NsaVersion::from_repeat(columns[flux_idx].repeat)?;

        let nsaid = read_u32_col(&bytes, hdu, &columns, &by_name, "NSAID")?;
        let ra = read_f64_col(&bytes, hdu, &columns, &by_name, "RA")?;
        let dec = read_f64_col(&bytes, hdu, &columns, &by_name, "DEC")?;
        let z = read_f32_col(&bytes, hdu, &columns, &by_name, "Z")?;
        let th50 = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_TH50")?;
        let nser = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_N")?;
        let ba = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_BA")?;
        let phi = read_f32_col(&bytes, hdu, &columns, &by_name, "SERSIC_PHI")?;
        let flux = read_f32_band_array(&bytes, hdu, &columns, &by_name, flux_name, version)?;
        let flux_ivar =
            read_f32_band_array(&bytes, hdu, &columns, &by_name, flux_ivar_name, version)?;

        let n = nsaid.len();
        for (label, len) in [
            ("RA", ra.len()),
            ("DEC", dec.len()),
            ("Z", z.len()),
            ("SERSIC_TH50", th50.len()),
            ("SERSIC_N", nser.len()),
            ("SERSIC_BA", ba.len()),
            ("SERSIC_PHI", phi.len()),
            (flux_name, flux.len()),
            (flux_ivar_name, flux_ivar.len()),
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

        Ok(Self { entries, version })
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

/// Return the first name from `candidates` that is present in `by_name`. Used
/// to handle column-name spelling differences across NSA versions (e.g. v0_1_2
/// has `SERSICFLUX`, v1_0_1 has `SERSIC_FLUX`). Errors include every candidate
/// so the failure message is debuggable when an unfamiliar release is loaded.
fn pick_name<'a>(by_name: &HashMap<&str, usize>, candidates: &[&'a str]) -> Result<&'a str> {
    for &name in candidates {
        if by_name.contains_key(name) {
            return Ok(name);
        }
    }
    Err(StarfieldError::DataError(format!(
        "NSA: none of the expected column names {:?} were present",
        candidates
    )))
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

/// Read a per-band float array column and materialize one `[f32; 7]` per row,
/// re-aligning v0_1_2's 5-band data into the canonical 7-slot in-memory layout
/// (FUV/NUV slots zeroed). The actual on-file repeat must match what the
/// detected version says (5 for v0_1_2, 7 for v1_0_1) — anything else is a
/// shape mismatch and bails.
fn read_f32_band_array(
    bytes: &[u8],
    hdu: &Hdu,
    columns: &[BinaryColumnDescriptor],
    by_name: &HashMap<&str, usize>,
    name: &str,
    version: NsaVersion,
) -> Result<Vec<[f32; N_BANDS]>> {
    let idx = col_index(by_name, name)?;
    let n_src = version.n_source_bands();
    if columns[idx].repeat != n_src {
        return Err(StarfieldError::DataError(format!(
            "NSA column `{}` has TFORM repeat {}, expected {} for {:?}",
            name, columns[idx].repeat, n_src, version
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
    if !flat.len().is_multiple_of(n_src) {
        return Err(StarfieldError::DataError(format!(
            "NSA column `{}` flattened length {} is not a multiple of {}",
            name,
            flat.len(),
            n_src
        )));
    }
    let n_rows = flat.len() / n_src;
    let mut out = Vec::with_capacity(n_rows);
    // v1_0_1: source layout is already FUV, NUV, u, g, r, i, z — copy verbatim.
    // v0_1_2: source layout is u, g, r, i, z — slot into indices 2..7, leaving
    //          FUV (0) and NUV (1) at zero so `ab_magnitude(0)`/`(1)` returns
    //          None for those rows.
    let dst_start = match version {
        NsaVersion::V1_0_1 => 0,
        NsaVersion::V0_1_2 => 2,
    };
    for chunk in flat.chunks_exact(n_src) {
        let mut a = [0f32; N_BANDS];
        a[dst_start..dst_start + n_src].copy_from_slice(chunk);
        out.push(a);
    }
    Ok(out)
}
