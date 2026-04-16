//! Generic in-memory catalog shared by all releases.

use std::collections::HashMap;
use std::path::Path;

use starfield::catalogs::{StarCatalog, StarData};
use starfield::Result;

use crate::common::reader::CsvSourceReader;
use crate::common::traits::{GaiaRelease, GaiaSource, Release};

/// In-memory Gaia catalog keyed by `source_id`, parameterized over a release marker.
///
/// Per-release newtypes (`Dr1Catalog`, `Dr2Catalog`, `Dr3Catalog`) wrap this so users
/// don't have to write turbofish at call sites.
#[derive(Debug)]
pub struct GaiaCatalogBase<R: GaiaRelease> {
    stars: HashMap<u64, R::Entry>,
    mag_limit: f64,
}

impl<R: GaiaRelease> GaiaCatalogBase<R> {
    pub fn new() -> Self {
        Self {
            stars: HashMap::new(),
            mag_limit: f64::INFINITY,
        }
    }

    /// Load a single `.csv` or `.csv.gz` file. Entries with `phot_g_mean_mag > mag_limit`
    /// are discarded as they stream past.
    pub fn from_csv_file(path: impl AsRef<Path>, mag_limit: f64) -> Result<Self> {
        let mut catalog = Self {
            stars: HashMap::new(),
            mag_limit,
        };
        let reader = CsvSourceReader::<R>::open(path, mag_limit)?;
        for entry in reader {
            let entry = entry?;
            let id = entry.core().source_id;
            catalog.stars.insert(id, entry);
        }
        Ok(catalog)
    }

    /// The magnitude cutoff used when populating this catalog.
    pub fn mag_limit(&self) -> f64 {
        self.mag_limit
    }

    /// Which Gaia release this catalog holds.
    pub fn release(&self) -> Release {
        R::RELEASE
    }

    /// Stars strictly brighter (smaller mag) than or equal to `magnitude`, by reference.
    pub fn brighter_than_ref(&self, magnitude: f64) -> Vec<&R::Entry> {
        self.stars
            .values()
            .filter(|e| e.core().phot_g_mean_mag <= magnitude)
            .collect()
    }

    /// Merge another catalog into this one; existing entries win on `source_id` collision.
    pub fn merge(&mut self, other: Self) {
        for (id, star) in other.stars {
            self.stars.entry(id).or_insert(star);
        }
        self.mag_limit = self.mag_limit.min(other.mag_limit);
    }

    /// Insert (mostly useful in tests and synthetic catalogs).
    pub fn insert(&mut self, entry: R::Entry) {
        self.stars.insert(entry.core().source_id, entry);
    }

    /// Mutable iterator over every entry. Used by per-release catalog helpers that
    /// splice in supplementary data (e.g. DR1 TGAS cross-ids).
    pub fn stars_iter_mut(&mut self) -> impl Iterator<Item = &mut R::Entry> {
        self.stars.values_mut()
    }
}

impl<R: GaiaRelease> Default for GaiaCatalogBase<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: GaiaRelease> StarCatalog for GaiaCatalogBase<R> {
    type Star = R::Entry;

    fn get_star(&self, id: usize) -> Option<&Self::Star> {
        self.stars.get(&(id as u64))
    }

    fn stars(&self) -> impl Iterator<Item = &Self::Star> {
        self.stars.values()
    }

    fn len(&self) -> usize {
        self.stars.len()
    }

    fn filter<F>(&self, predicate: F) -> Vec<&Self::Star>
    where
        F: Fn(&Self::Star) -> bool,
    {
        self.stars.values().filter(|s| predicate(s)).collect()
    }

    fn star_data(&self) -> impl Iterator<Item = StarData> + '_ {
        self.stars.values().map(|e| {
            let c = e.core();
            StarData::new(c.source_id, c.ra, c.dec, c.phot_g_mean_mag, e.b_v())
        })
    }

    fn filter_star_data<F>(&self, predicate: F) -> Vec<StarData>
    where
        F: Fn(&StarData) -> bool,
    {
        self.star_data().filter(|s| predicate(s)).collect()
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

fn unit_vec(ra_deg: f64, dec_deg: f64) -> nalgebra::Vector3<f64> {
    let ra = ra_deg.to_radians();
    let dec = dec_deg.to_radians();
    nalgebra::Vector3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin())
}
