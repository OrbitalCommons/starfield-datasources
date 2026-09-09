//! Karkoschka full-disk albedo spectra of the jovian planets and Titan.
//!
//! Parses PDS Atmospheres volume `gbat_0001`, dataset
//! `ESO-J/S/N/U-SPECTROPHOTOMETER-4-V2.0` (DOI `10.17189/2bp8-k793`) —
//! Boller & Chivens spectrograph observations from the European Southern
//! Observatory, reduced by Erich Karkoschka.
//!
//! Three products exist. The low-resolution 1995 table is embedded because it
//! spans the whole silicon detector response; the other two are downloaded on
//! demand.
//!
//! | Product | Range | Sampling | Bodies |
//! |---|---|---|---|
//! | [`Product::Low1995`] | 300–1050 nm | 0.4 nm | Jupiter, Saturn, Uranus, Neptune, Titan |
//! | [`Product::High1995`] | 520–995 nm | 0.1 nm | Jupiter, Saturn, Uranus |
//! | [`Product::Table1993`] | 300–1000 nm | 0.4 nm | Jupiter, Saturn, Uranus, Neptune, Titan |

use std::fs;
use std::path::{Path, PathBuf};

use starfield::{Result, StarfieldError};
use starfield_datastore::{Artifact, ArtifactKey, Datastore, Freshness, Provenance, Source};

use crate::albedo::{AlbedoKind, SpectralAlbedo};
use crate::body::SpectralBody;

/// The 1995 low-resolution table, vendored verbatim from the PDS volume.
const EMBEDDED_LOW_1995: &str = include_str!("../data/1995low.tab");

/// Base URL of the `gbat_0001` data directory.
pub const PDS_DATA_BASE_URL: &str = "https://pds-atmospheres.nmsu.edu/PDS/data/gbat_0001/data/";

/// Artifact key prefix. Archive-shaped rather than URL-shaped, so a relocated
/// upstream changes a [`Source`] and not the cache layout or anyone's pin.
const KEY_PREFIX: &str = "pds/gbat_0001";

/// Phase angle of the Jupiter column in the 1995 tables, per the PDS label.
const JUPITER_PHASE_DEG: f64 = 6.8;
/// Phase angle of the Saturn column in the 1995 tables, per the PDS label.
const SATURN_PHASE_DEG: f64 = 5.7;

/// One of the three tables in the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    /// `1995low.tab` — 300–1050 nm at 0.4 nm sampling, 1 nm resolution.
    Low1995,
    /// `1995high.tab` — 520–995 nm at 0.1 nm sampling, 0.4 nm resolution.
    /// Carries no Neptune or Titan column.
    High1995,
    /// `1993.tab` — 300–1000 nm at 0.4 nm sampling, 1 nm resolution.
    Table1993,
}

impl Product {
    /// Archive file name.
    pub fn file_name(&self) -> &'static str {
        match self {
            Product::Low1995 => "1995low.tab",
            Product::High1995 => "1995high.tab",
            Product::Table1993 => "1993.tab",
        }
    }

    /// Full download URL.
    pub fn url(&self) -> String {
        format!("{}{}", PDS_DATA_BASE_URL, self.file_name())
    }

    /// Row count declared by the PDS label. Used to validate a parse.
    pub fn expected_rows(&self) -> usize {
        match self {
            Product::Low1995 => 1875,
            Product::High1995 => 4750,
            Product::Table1993 => 1750,
        }
    }

    /// Bodies with an albedo column, in column order.
    pub fn bodies(&self) -> &'static [SpectralBody] {
        match self {
            Product::Low1995 | Product::Table1993 => &[
                SpectralBody::Jupiter,
                SpectralBody::Saturn,
                SpectralBody::Uranus,
                SpectralBody::Neptune,
                SpectralBody::Titan,
            ],
            Product::High1995 => &[
                SpectralBody::Jupiter,
                SpectralBody::Saturn,
                SpectralBody::Uranus,
            ],
        }
    }

    /// This product as a datastore [`Artifact`].
    ///
    /// `Immutable`: a numbered PDS volume file never changes. The default
    /// content check rejects an HTML body, which is what an archive soft-404 or
    /// a captive portal actually returns — verified against a live USGS
    /// soft-404 during integration.
    pub fn artifact(&self) -> Artifact {
        Artifact::new(
            ArtifactKey::new(format!("{KEY_PREFIX}/{}", self.file_name()))
                .expect("static key is well-formed"),
            vec![Source::new(self.url())],
        )
        .with_freshness(Freshness::Immutable)
        .with_provenance(Provenance {
            description: format!(
                "Karkoschka spectrophotometry of the jovian planets and Titan, \
                 PDS Atmospheres volume gbat_0001, {}",
                self.file_name()
            ),
            license: "public-domain".into(),
            citation: Some("Karkoschka 1998, Icarus 133, 134; PDS DOI 10.17189/2bp8-k793".into()),
        })
    }

    /// Provenance string recorded on every spectrum from this product.
    fn source(&self) -> &'static str {
        match self {
            Product::Low1995 => "Karkoschka 1998, PDS gbat_0001 1995low.tab",
            Product::High1995 => "Karkoschka 1998, PDS gbat_0001 1995high.tab",
            Product::Table1993 => "Karkoschka 1994, PDS gbat_0001 1993.tab",
        }
    }
}

/// A parsed Karkoschka table.
///
/// Holds the wavelength grid, the methane absorption coefficients, and one
/// albedo column per body.
#[derive(Debug, Clone)]
pub struct KarkoschkaTable {
    product: Product,
    vacuum_nm: Vec<f64>,
    air_nm: Vec<f64>,
    methane_absorption: Vec<f64>,
    albedo_columns: Vec<Vec<f64>>,
}

impl KarkoschkaTable {
    /// Parse the embedded 1995 low-resolution table. No network, no I/O.
    pub fn load_embedded() -> Result<Self> {
        Self::parse(Product::Low1995, EMBEDDED_LOW_1995)
    }

    /// Parse a product from a file already on disk.
    pub fn from_file<P: AsRef<Path>>(product: Product, path: P) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        Self::parse(product, &text)
    }

    /// Resolve a product through `starfield-datastore` and parse it.
    ///
    /// Resolution is local cache, then the organisation mirror, and upstream
    /// only when `STARFIELD_ALLOW_UPSTREAM=1` — so a working checkout depends on
    /// one service rather than on the PDS Atmospheres node being up. Repeat
    /// calls are served from disk.
    ///
    /// Use [`KarkoschkaTable::download_with`] to supply a configured store.
    pub fn download(product: Product) -> Result<Self> {
        let store = Datastore::from_env().map_err(to_starfield)?;
        Self::download_with(&store, product)
    }

    /// As [`KarkoschkaTable::download`], against a caller-configured store.
    pub fn download_with(store: &Datastore, product: Product) -> Result<Self> {
        let path = store.get(&product.artifact()).map_err(to_starfield)?;
        Self::from_file(product, path)
    }

    /// Local path of a product if it is already cached, without fetching.
    pub fn cached_path(store: &Datastore, product: Product) -> Option<PathBuf> {
        store.peek(&product.artifact().key)
    }

    /// Parse the fixed-column ASCII body of a PDS table.
    ///
    /// Rows are `vacuum_nm  air_nm  ch4_coefficient` followed by one albedo per
    /// body. The PDS label declares byte offsets, but every column is separated
    /// by at least one space in all three products, so whitespace splitting is
    /// equivalent and tolerates the trailing-blank variations between them.
    pub fn parse(product: Product, text: &str) -> Result<Self> {
        let expected_columns = 3 + product.bodies().len();
        let mut vacuum_nm = Vec::with_capacity(product.expected_rows());
        let mut air_nm = Vec::with_capacity(product.expected_rows());
        let mut methane_absorption = Vec::with_capacity(product.expected_rows());
        let mut albedo_columns =
            vec![Vec::with_capacity(product.expected_rows()); product.bodies().len()];

        for (line_no, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != expected_columns {
                return Err(StarfieldError::DataError(format!(
                    "{}: line {} has {} columns, expected {}",
                    product.file_name(),
                    line_no + 1,
                    fields.len(),
                    expected_columns
                )));
            }
            let parse_at = |i: usize| -> Result<f64> {
                fields[i].parse::<f64>().map_err(|e| {
                    StarfieldError::DataError(format!(
                        "{}: line {} column {}: cannot parse {:?}: {}",
                        product.file_name(),
                        line_no + 1,
                        i + 1,
                        fields[i],
                        e
                    ))
                })
            };

            vacuum_nm.push(parse_at(0)?);
            air_nm.push(parse_at(1)?);
            methane_absorption.push(parse_at(2)?);
            for (col, values) in albedo_columns.iter_mut().enumerate() {
                values.push(parse_at(3 + col)?);
            }
        }

        if vacuum_nm.len() != product.expected_rows() {
            return Err(StarfieldError::DataError(format!(
                "{}: parsed {} rows, PDS label declares {}",
                product.file_name(),
                vacuum_nm.len(),
                product.expected_rows()
            )));
        }

        Ok(Self {
            product,
            vacuum_nm,
            air_nm,
            methane_absorption,
            albedo_columns,
        })
    }

    /// Which product this table came from.
    pub fn product(&self) -> Product {
        self.product
    }

    /// Number of wavelength samples.
    pub fn len(&self) -> usize {
        self.vacuum_nm.len()
    }

    /// Whether the table is empty. A successful parse never is.
    pub fn is_empty(&self) -> bool {
        self.vacuum_nm.is_empty()
    }

    /// Vacuum wavelength grid, nanometres.
    pub fn vacuum_nm(&self) -> &[f64] {
        &self.vacuum_nm
    }

    /// Air wavelength grid, nanometres.
    pub fn air_nm(&self) -> &[f64] {
        &self.air_nm
    }

    /// Methane absorption coefficients, in (km-amagat)⁻¹, on the vacuum grid.
    ///
    /// Determined indirectly from the planetary spectra by the method of
    /// Karkoschka & Tomasko (1992), *Icarus* 97, 161–181.
    pub fn methane_absorption(&self) -> &[f64] {
        &self.methane_absorption
    }

    /// The albedo spectrum for `body`, or `None` if this product has no column
    /// for it — [`Product::High1995`] carries no Neptune or Titan.
    ///
    /// The returned [`SpectralAlbedo`] is on the **vacuum** wavelength grid and
    /// records the correct [`AlbedoKind`] for the column: the PDS label gives
    /// the Jupiter and Saturn columns as full-disk albedos at 6.8° and 5.7°
    /// phase respectively, and the Uranus, Neptune and Titan columns as
    /// geometric albedos.
    ///
    /// Saturn's column is the globe at zero ring tilt — the rings are excluded.
    pub fn albedo(&self, body: SpectralBody) -> Option<SpectralAlbedo> {
        let col = self.product.bodies().iter().position(|&b| b == body)?;
        Some(SpectralAlbedo::new(
            body,
            albedo_kind(body),
            self.product.source(),
            self.vacuum_nm.clone(),
            self.albedo_columns[col].clone(),
        ))
    }

    /// Every albedo spectrum in this product, in column order.
    pub fn albedos(&self) -> Vec<SpectralAlbedo> {
        self.product
            .bodies()
            .iter()
            .filter_map(|&b| self.albedo(b))
            .collect()
    }
}

/// Map a datastore error into the workspace error type.
///
/// The datastore deliberately carries its own error type — it must not depend
/// on `starfield` — so the message is preserved rather than the variant.
fn to_starfield(e: starfield_datastore::DatastoreError) -> StarfieldError {
    StarfieldError::DataError(e.to_string())
}

/// What the archive's column for `body` actually measures, per the PDS label.
fn albedo_kind(body: SpectralBody) -> AlbedoKind {
    match body {
        SpectralBody::Jupiter => AlbedoKind::FullDisk {
            phase_angle_deg: JUPITER_PHASE_DEG,
        },
        SpectralBody::Saturn => AlbedoKind::FullDisk {
            phase_angle_deg: SATURN_PHASE_DEG,
        },
        SpectralBody::Uranus | SpectralBody::Neptune | SpectralBody::Titan => AlbedoKind::Geometric,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store guaranteed to reach upstream on every call: an empty cache root,
    /// no mirror, and upstream enabled programmatically rather than from the
    /// environment.
    ///
    /// The two live tests below are **upstream-rot canaries** — they exist to
    /// detect that a PDS URL has died or relocated, which is not hypothetical
    /// (LP DAAC relocated its paths this year). Serving them from the mirror or
    /// a warm cache would defeat them: the mirror would go on returning a copy
    /// of a product whose upstream URL died years ago and the test would pass
    /// forever.
    ///
    /// `allow_upstream` is set here rather than read from `STARFIELD_ALLOW_UPSTREAM`
    /// so that a missing variable cannot turn the canary into a silent skip.
    fn cold_upstream_store(dir: &std::path::Path) -> Datastore {
        Datastore::builder()
            .cache_root(dir.to_path_buf())
            .without_mirror()
            .allow_upstream(true)
            .build()
            .expect("build cold store")
    }

    /// The first three and last rows of the vendored 1995low.tab, verbatim.
    const EXCERPT: &str = " 300.4  300.31   .0000 .2128 .2532 .5306 .6933 .0431\n\
                            300.8  300.71   .0000 .2084 .2359 .5200 .6152 .0507\n\
                            301.2  301.11   .0000 .2215 .2404 .5353 .5610 .0616\n";

    #[test]
    fn parses_a_real_excerpt() {
        // Product::Low1995 declares 1875 rows, so parse the excerpt through the
        // column logic without the row-count gate by checking fields directly.
        let rows: Vec<Vec<f64>> = EXCERPT
            .lines()
            .map(|l| {
                l.split_whitespace()
                    .map(|f| f.parse::<f64>().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 8);
        assert_eq!(rows[0][0], 300.4);
        assert_eq!(rows[0][3], 0.2128);
    }

    #[test]
    fn embedded_table_matches_the_pds_label() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        assert_eq!(t.len(), Product::Low1995.expected_rows());
        assert_eq!(t.vacuum_nm().len(), 1875);
        assert_eq!(t.methane_absorption().len(), 1875);
    }

    #[test]
    fn embedded_wavelength_grid_spans_the_declared_range() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        let v = t.vacuum_nm();
        // The label says 300-1050 nm at 0.4 nm sampling.
        assert_eq!(v[0], 300.4);
        assert!((v[v.len() - 1] - 1050.0).abs() < 0.5);
        for w in v.windows(2) {
            assert!((w[1] - w[0] - 0.4).abs() < 1e-9, "sampling is 0.4 nm");
        }
    }

    #[test]
    fn air_wavelengths_are_shorter_than_vacuum() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        for (v, a) in t.vacuum_nm().iter().zip(t.air_nm()) {
            assert!(a < v, "air wavelength {a} should be below vacuum {v}");
        }
    }

    #[test]
    fn first_row_values_match_the_archive() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        assert_eq!(t.vacuum_nm()[0], 300.4);
        assert_eq!(t.air_nm()[0], 300.31);
        assert_eq!(t.methane_absorption()[0], 0.0);
        assert_eq!(
            t.albedo(SpectralBody::Jupiter).unwrap().at_nm(300.4),
            Some(0.2128)
        );
        assert_eq!(
            t.albedo(SpectralBody::Saturn).unwrap().at_nm(300.4),
            Some(0.2532)
        );
        assert_eq!(
            t.albedo(SpectralBody::Uranus).unwrap().at_nm(300.4),
            Some(0.5306)
        );
        assert_eq!(
            t.albedo(SpectralBody::Neptune).unwrap().at_nm(300.4),
            Some(0.6933)
        );
        assert_eq!(
            t.albedo(SpectralBody::Titan).unwrap().at_nm(300.4),
            Some(0.0431)
        );
    }

    #[test]
    fn albedo_kinds_follow_the_pds_label() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        assert_eq!(
            t.albedo(SpectralBody::Jupiter).unwrap().kind(),
            AlbedoKind::FullDisk {
                phase_angle_deg: 6.8
            }
        );
        assert_eq!(
            t.albedo(SpectralBody::Saturn).unwrap().kind(),
            AlbedoKind::FullDisk {
                phase_angle_deg: 5.7
            }
        );
        assert_eq!(
            t.albedo(SpectralBody::Uranus).unwrap().kind(),
            AlbedoKind::Geometric
        );
        assert_eq!(
            t.albedo(SpectralBody::Titan).unwrap().kind(),
            AlbedoKind::Geometric
        );
    }

    #[test]
    fn every_embedded_albedo_is_physical() {
        let t = KarkoschkaTable::load_embedded().unwrap();
        for spectrum in t.albedos() {
            for (nm, a) in spectrum.samples() {
                assert!(
                    (0.0..=1.5).contains(&a),
                    "{} albedo {a} at {nm} nm is out of range",
                    spectrum.body().name()
                );
            }
        }
    }

    #[test]
    fn high_res_product_has_no_neptune_or_titan_column() {
        assert_eq!(Product::High1995.bodies().len(), 3);
        assert!(!Product::High1995.bodies().contains(&SpectralBody::Neptune));
        assert!(!Product::High1995.bodies().contains(&SpectralBody::Titan));
    }

    #[test]
    fn parse_rejects_a_short_row() {
        let bad = " 300.4  300.31   .0000 .2128\n";
        let err = KarkoschkaTable::parse(Product::Low1995, bad).unwrap_err();
        assert!(err.to_string().contains("expected 8"), "{err}");
    }

    #[test]
    fn parse_rejects_a_truncated_table() {
        let err = KarkoschkaTable::parse(Product::Low1995, EXCERPT).unwrap_err();
        assert!(err.to_string().contains("declares 1875"), "{err}");
    }

    #[test]
    fn artifact_keys_are_archive_shaped_not_url_shaped() {
        // A relocated upstream must change a Source, not the cache layout or a
        // pinned digest. LP DAAC relocated its paths this year, so this is the
        // failure mode being designed against rather than a hypothetical.
        let a = Product::Low1995.artifact();
        assert_eq!(a.key.as_str(), "pds/gbat_0001/1995low.tab");
        assert!(!a.key.as_str().contains("://"));
        assert_eq!(a.sources.len(), 1);
        assert!(a.sources[0].url.contains("pds-atmospheres.nmsu.edu"));
        assert_eq!(a.freshness, Freshness::Immutable);
    }

    #[test]
    fn urls_point_at_the_pds_volume() {
        assert_eq!(
            Product::Low1995.url(),
            "https://pds-atmospheres.nmsu.edu/PDS/data/gbat_0001/data/1995low.tab"
        );
    }

    #[test]
    #[ignore = "live upstream; bypasses the mirror. Detects archive rot, so it must not be re-pointed at the datastore"]
    fn downloads_and_parses_the_high_resolution_product() {
        let dir = tempfile::tempdir().unwrap();
        let store = cold_upstream_store(dir.path());
        let t = KarkoschkaTable::download_with(&store, Product::High1995).unwrap();
        assert_eq!(t.len(), 4750);
        let (lo, hi) = t.albedo(SpectralBody::Jupiter).unwrap().range_nm();
        assert!(lo >= 520.0 && hi <= 995.5, "range {lo}-{hi}");
    }

    #[test]
    #[ignore = "live upstream; bypasses the mirror. Detects archive rot, so it must not be re-pointed at the datastore"]
    fn downloaded_1993_table_agrees_with_the_embedded_1995_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = cold_upstream_store(dir.path());
        let old = KarkoschkaTable::download_with(&store, Product::Table1993).unwrap();
        let new = KarkoschkaTable::load_embedded().unwrap();
        // Independent reductions three years apart: Uranus is spectrally stable,
        // so the two should agree to a few percent away from strong methane bands.
        let a = old.albedo(SpectralBody::Uranus).unwrap();
        let b = new.albedo(SpectralBody::Uranus).unwrap();
        let (x, y) = (
            a.mean_over(400.0, 500.0).unwrap(),
            b.mean_over(400.0, 500.0).unwrap(),
        );
        assert!((x - y).abs() < 0.05, "1993 {x} vs 1995 {y}");
    }
}
