//! Endmember surface reflectance spectra for resolved-body rendering.
//!
//! A global albedo map is one number per texel, but a real surface is a mixture
//! of a few materials whose *colours* differ. Multiplying a monochrome map by a
//! single disk-integrated spectrum gives a body whose colour is uniform and only
//! its brightness varies — visibly wrong for Mars and the Moon, the two bodies
//! most likely to be rendered large. Decomposing each texel into a few
//! endmembers with distinct spectra is what makes colour vary across the disk.
//!
//! This crate supplies the endmember spectra. The mixing fractions come from the
//! map crate; the radiometry stays with the consumer.
//!
//! # Source
//!
//! USGS Spectral Library Version 7 (Kokaly et al. 2017, USGS Data Series 1035,
//! doi:10.5066/F7RR1WDJ), a public-domain laboratory-measured library.
//! Reflectance is AREF (absolute reflectance), dimensionless.
//! `scripts/build_table.py` regenerates the embedded table from the archive and
//! records the curation.
//!
//! # Coverage is per-endmember and is not hidden
//!
//! The endmembers do not share a wavelength range: splib07 measured them on
//! different spectrometers, and deleted channels it could not calibrate. Most
//! span the silicon detector range, but [`Endmember::WaterIce`] **starts at
//! 859 nm** — splib07's only water-ice spectrum has no visible coverage at all.
//!
//! Rather than pad that gap, every accessor reports honestly: asking for a
//! visible-band mean of water ice returns `None`. A consumer that needs visible
//! ice must source it elsewhere, and this crate makes that obvious at the call
//! site instead of returning a fabricated number.
//!
//! # Example
//!
//! ```
//! use starfield_reflectance_library::{Endmember, EndmemberMix, ReflectanceLibrary};
//!
//! let library = ReflectanceLibrary::load_embedded()?;
//!
//! // Vegetation's red edge: a steep rise between 680 and 750 nm.
//! let leaf = library.get(Endmember::GreenVegetation).unwrap();
//! let ratio = leaf.at_nm(750.0).unwrap() / leaf.at_nm(680.0).unwrap();
//! assert!(ratio > 4.0, "red edge ratio {ratio}");
//!
//! // A half-snow, half-vegetation texel, averaged over a red band.
//! let mix = EndmemberMix::new(vec![
//!     (Endmember::Snow, 0.5),
//!     (Endmember::GreenVegetation, 0.5),
//! ]);
//! let mean = mix.band_mean(&library, 600.0, 700.0).unwrap();
//! println!("mixed reflectance over 600-700 nm: {mean:.3}");
//! # Ok::<(), starfield::StarfieldError>(())
//! ```

use std::collections::HashMap;

use starfield::{Result, StarfieldError};
use starfield_datasource_utils::SampledCurve;

const EMBEDDED_CSV: &str = include_str!("../data/splib07_endmembers.csv");

/// A material whose reflectance spectrum this crate ships.
///
/// This enum is the stable identifier shared between the reflectance library and
/// the map crate: a map stores mixing fractions keyed on these variants, and
/// resolves them here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Endmember {
    /// Open-ocean seawater, low chlorophyll.
    OpenOcean,
    /// Coastal seawater with chlorophyll.
    CoastalWater,
    /// Green aspen leaf — the canonical red-edge spectrum.
    GreenVegetation,
    /// Dry golden grass; non-photosynthetic vegetation.
    DryVegetation,
    /// Uncontaminated beach sand.
    Sand,
    /// Melting snow. splib07 has no dry or fresh snow spectrum, so this is
    /// darker in the near-infrared than fresh powder would be.
    Snow,
    /// Water ice at 77 K. **Covers 859 nm and longward only** — see the module
    /// docs.
    WaterIce,
    /// Fresh basalt; the closest laboratory analogue in this library to lunar
    /// mare and Mars dark terrain.
    FreshBasalt,
    /// Weathered basalt.
    WeatheredBasalt,
}

impl Endmember {
    /// Every endmember in the library.
    pub const ALL: [Endmember; 9] = [
        Endmember::OpenOcean,
        Endmember::CoastalWater,
        Endmember::GreenVegetation,
        Endmember::DryVegetation,
        Endmember::Sand,
        Endmember::Snow,
        Endmember::WaterIce,
        Endmember::FreshBasalt,
        Endmember::WeatheredBasalt,
    ];

    /// The identifier used in the embedded table and on the wire.
    pub fn id(&self) -> &'static str {
        match self {
            Endmember::OpenOcean => "OpenOcean",
            Endmember::CoastalWater => "CoastalWater",
            Endmember::GreenVegetation => "GreenVegetation",
            Endmember::DryVegetation => "DryVegetation",
            Endmember::Sand => "Sand",
            Endmember::Snow => "Snow",
            Endmember::WaterIce => "WaterIce",
            Endmember::FreshBasalt => "FreshBasalt",
            Endmember::WeatheredBasalt => "WeatheredBasalt",
        }
    }

    /// Parse an [`Endmember::id`].
    pub fn from_id(id: &str) -> Option<Endmember> {
        Endmember::ALL.into_iter().find(|e| e.id() == id)
    }
}

/// One endmember's measured reflectance against wavelength.
#[derive(Debug, Clone, PartialEq)]
pub struct Reflectance {
    endmember: Endmember,
    curve: SampledCurve,
}

impl Reflectance {
    /// Which material this describes.
    pub fn endmember(&self) -> Endmember {
        self.endmember
    }

    /// Inclusive wavelength coverage, nm. Differs per endmember.
    pub fn range_nm(&self) -> (f64, f64) {
        self.curve.range_nm()
    }

    /// Number of measured channels.
    pub fn len(&self) -> usize {
        self.curve.len()
    }

    /// Always false — a parsed reflectance has at least one channel.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Reflectance at `nm`, linearly interpolated. `None` outside coverage.
    pub fn at_nm(&self, nm: f64) -> Option<f64> {
        self.curve.at(nm)
    }

    /// Mean reflectance over `[lo_nm, hi_nm]`. `None` unless wholly covered.
    ///
    /// Call this once per band per endmember and reuse the result: the
    /// per-sub-sample cost then reduces to a weighted sum with no spectral work.
    pub fn band_mean(&self, lo_nm: f64, hi_nm: f64) -> Option<f64> {
        self.curve.mean_over(lo_nm, hi_nm)
    }

    /// The measured samples, as `(wavelength_nm, reflectance)`.
    pub fn samples(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.curve.samples()
    }

    /// The underlying curve, for consumers sharing one integration path.
    pub fn curve(&self) -> &SampledCurve {
        &self.curve
    }
}

/// The embedded endmember spectra.
#[derive(Debug, Clone)]
pub struct ReflectanceLibrary {
    spectra: HashMap<Endmember, Reflectance>,
}

impl ReflectanceLibrary {
    /// Parse the embedded table. No network, no I/O.
    pub fn load_embedded() -> Result<Self> {
        Self::parse(EMBEDDED_CSV)
    }

    /// Parse a table in the embedded CSV's format.
    pub fn parse(text: &str) -> Result<Self> {
        let mut columns: HashMap<Endmember, (Vec<f64>, Vec<f64>)> = HashMap::new();

        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("endmember") {
                continue;
            }
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() != 3 {
                return Err(StarfieldError::DataError(format!(
                    "reflectance line {}: expected 3 columns, got {}",
                    line_no + 1,
                    fields.len()
                )));
            }
            let endmember = Endmember::from_id(fields[0]).ok_or_else(|| {
                StarfieldError::DataError(format!(
                    "reflectance line {}: unknown endmember {:?}",
                    line_no + 1,
                    fields[0]
                ))
            })?;
            let parse = |s: &str, what: &str| -> Result<f64> {
                s.trim().parse::<f64>().map_err(|e| {
                    StarfieldError::DataError(format!(
                        "reflectance line {}: bad {what}: {e}",
                        line_no + 1
                    ))
                })
            };
            let entry = columns.entry(endmember).or_default();
            entry.0.push(parse(fields[1], "wavelength")?);
            entry.1.push(parse(fields[2], "reflectance")?);
        }

        let mut spectra = HashMap::new();
        for (endmember, (wavelengths, values)) in columns {
            let curve = SampledCurve::new(wavelengths, values)
                .map_err(|e| StarfieldError::DataError(format!("{}: {e}", endmember.id())))?;
            spectra.insert(endmember, Reflectance { endmember, curve });
        }

        if spectra.is_empty() {
            return Err(StarfieldError::DataError(
                "reflectance library is empty".into(),
            ));
        }
        Ok(Self { spectra })
    }

    /// The spectrum for `endmember`, if the library has one.
    pub fn get(&self, endmember: Endmember) -> Option<&Reflectance> {
        self.spectra.get(&endmember)
    }

    /// How many endmembers are loaded.
    pub fn len(&self) -> usize {
        self.spectra.len()
    }

    /// Always false — a parsed library has at least one endmember.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Every loaded endmember, in a stable order.
    pub fn endmembers(&self) -> Vec<Endmember> {
        let mut ids: Vec<Endmember> = self.spectra.keys().copied().collect();
        ids.sort();
        ids
    }
}

/// A texel's material composition: endmembers and their fractional weights.
///
/// The map crate returns one of these per sample rather than a bare albedo, so
/// there is a single code path whether a body has one material or several. A
/// uniform surface is [`EndmemberMix::uniform`], a one-endmember mix.
#[derive(Debug, Clone, PartialEq)]
pub struct EndmemberMix {
    weights: Vec<(Endmember, f64)>,
}

impl EndmemberMix {
    /// Build from `(endmember, weight)` pairs. Weights are used as given and are
    /// not renormalised — see [`EndmemberMix::total_weight`].
    pub fn new(weights: Vec<(Endmember, f64)>) -> Self {
        Self { weights }
    }

    /// A pure surface of one material.
    pub fn uniform(endmember: Endmember) -> Self {
        Self {
            weights: vec![(endmember, 1.0)],
        }
    }

    /// The `(endmember, weight)` pairs.
    pub fn weights(&self) -> &[(Endmember, f64)] {
        &self.weights
    }

    /// Number of contributing endmembers.
    pub fn len(&self) -> usize {
        self.weights.len()
    }

    /// Whether the mix has no contributions.
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }

    /// Sum of the weights.
    ///
    /// Not forced to 1. A mix covering only part of a texel — a partially
    /// cloud-covered pixel, say — is legitimately short, and silently
    /// renormalising it would hide the missing fraction.
    pub fn total_weight(&self) -> f64 {
        self.weights.iter().map(|(_, w)| w).sum()
    }

    /// Weighted reflectance at `nm`.
    ///
    /// `None` if any contributing endmember lacks coverage at `nm` — a mix
    /// evaluated over a partial subset of its materials is not the reflectance
    /// of the mix, and returning it would understate the result by however much
    /// was dropped.
    pub fn reflectance_at(&self, library: &ReflectanceLibrary, nm: f64) -> Option<f64> {
        let mut total = 0.0;
        for &(endmember, weight) in &self.weights {
            total += weight * library.get(endmember)?.at_nm(nm)?;
        }
        Some(total)
    }

    /// Weighted mean reflectance over `[lo_nm, hi_nm]`.
    ///
    /// `None` if any contributing endmember does not fully cover the band. This
    /// is the call to make once per band per render; the per-sub-sample loop then
    /// reduces to a weighted sum over precomputed values.
    pub fn band_mean(&self, library: &ReflectanceLibrary, lo_nm: f64, hi_nm: f64) -> Option<f64> {
        let mut total = 0.0;
        for &(endmember, weight) in &self.weights {
            total += weight * library.get(endmember)?.band_mean(lo_nm, hi_nm)?;
        }
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> ReflectanceLibrary {
        ReflectanceLibrary::load_embedded().unwrap()
    }

    #[test]
    fn every_endmember_is_present() {
        let l = library();
        assert_eq!(l.len(), Endmember::ALL.len());
        for e in Endmember::ALL {
            assert!(l.get(e).is_some(), "{} missing", e.id());
        }
    }

    #[test]
    fn ids_round_trip() {
        for e in Endmember::ALL {
            assert_eq!(Endmember::from_id(e.id()), Some(e));
        }
        assert_eq!(Endmember::from_id("Unobtainium"), None);
    }

    #[test]
    fn reflectances_are_physical() {
        let l = library();
        for e in Endmember::ALL {
            for (nm, r) in l.get(e).unwrap().samples() {
                assert!(
                    (0.0..=1.0).contains(&r),
                    "{} reflectance {r} at {nm} nm is outside [0,1]",
                    e.id()
                );
            }
        }
    }

    #[test]
    fn vegetation_has_a_red_edge() {
        // The single most recognisable feature in terrestrial remote sensing:
        // chlorophyll absorbs hard in the red and the leaf scatters strongly in
        // the near-infrared, so reflectance jumps by a factor of several between
        // 680 and 750 nm. A wavelength-registration error would destroy this.
        let l = library();
        let leaf = l.get(Endmember::GreenVegetation).unwrap();
        let ratio = leaf.at_nm(750.0).unwrap() / leaf.at_nm(680.0).unwrap();
        assert!(ratio > 4.0, "red edge ratio {ratio}");
        // Dry vegetation has lost its chlorophyll and so has no red edge.
        let dry = l.get(Endmember::DryVegetation).unwrap();
        let dry_ratio = dry.at_nm(750.0).unwrap() / dry.at_nm(680.0).unwrap();
        assert!(dry_ratio < 2.0, "dry vegetation ratio {dry_ratio}");
    }

    #[test]
    fn snow_is_bright_in_the_visible_and_dark_in_the_swir() {
        let l = library();
        let snow = l.get(Endmember::Snow).unwrap();
        assert!(snow.at_nm(500.0).unwrap() > 0.7);
        assert!(snow.at_nm(1600.0).unwrap() < 0.1);
        assert!(snow.at_nm(500.0).unwrap() > 10.0 * snow.at_nm(1600.0).unwrap());
    }

    #[test]
    fn water_is_dark_and_blue() {
        let l = library();
        let ocean = l.get(Endmember::OpenOcean).unwrap();
        for nm in [450.0, 650.0, 850.0] {
            assert!(
                ocean.at_nm(nm).unwrap() < 0.1,
                "ocean too bright at {nm} nm"
            );
        }
        assert!(ocean.at_nm(450.0).unwrap() > ocean.at_nm(650.0).unwrap());
    }

    #[test]
    fn basalt_is_dark_and_spectrally_flat() {
        let l = library();
        let basalt = l.get(Endmember::FreshBasalt).unwrap();
        let v = basalt.at_nm(550.0).unwrap();
        let nir = basalt.at_nm(900.0).unwrap();
        assert!(v < 0.2, "fresh basalt visible reflectance {v}");
        assert!((nir / v) < 2.0, "basalt is not flat: {nir} vs {v}");
        // Weathering brightens it.
        let weathered = l.get(Endmember::WeatheredBasalt).unwrap();
        assert!(weathered.at_nm(550.0).unwrap() > v);
    }

    #[test]
    fn water_ice_has_no_visible_coverage_and_says_so() {
        // splib07's only water-ice spectrum starts at 859 nm. The library does
        // not pad the gap; it reports the absence.
        let l = library();
        let ice = l.get(Endmember::WaterIce).unwrap();
        let (lo, _) = ice.range_nm();
        assert!(lo > 800.0, "water ice unexpectedly starts at {lo} nm");
        assert_eq!(ice.at_nm(550.0), None);
        assert_eq!(ice.band_mean(400.0, 700.0), None);
        // It is usable where it does have coverage.
        assert!(ice.band_mean(1000.0, 1100.0).is_some());
    }

    #[test]
    fn most_endmembers_span_the_silicon_response() {
        let l = library();
        for e in Endmember::ALL {
            if e == Endmember::WaterIce {
                continue;
            }
            let r = l.get(e).unwrap();
            assert!(
                r.band_mean(400.0, 900.0).is_some(),
                "{} does not cover 400-900 nm; range is {:?}",
                e.id(),
                r.range_nm()
            );
        }
    }

    #[test]
    fn a_uniform_mix_equals_its_endmember() {
        let l = library();
        let mix = EndmemberMix::uniform(Endmember::Sand);
        let direct = l.get(Endmember::Sand).unwrap().at_nm(600.0).unwrap();
        assert!((mix.reflectance_at(&l, 600.0).unwrap() - direct).abs() < 1e-12);
        assert_eq!(mix.total_weight(), 1.0);
    }

    #[test]
    fn a_mix_lies_between_its_components() {
        let l = library();
        let snow = l.get(Endmember::Snow).unwrap().at_nm(600.0).unwrap();
        let basalt = l.get(Endmember::FreshBasalt).unwrap().at_nm(600.0).unwrap();
        let mix = EndmemberMix::new(vec![(Endmember::Snow, 0.5), (Endmember::FreshBasalt, 0.5)]);
        let value = mix.reflectance_at(&l, 600.0).unwrap();
        assert!(
            value > basalt && value < snow,
            "{basalt} < {value} < {snow}"
        );
        assert!((value - 0.5 * (snow + basalt)).abs() < 1e-12);
    }

    #[test]
    fn a_mix_is_none_if_any_component_lacks_coverage() {
        // Half sand, half water ice, evaluated in the visible: sand has data and
        // ice does not, so the mix is not evaluable. Returning sand's half alone
        // would understate the result by whatever the ice contributes.
        let l = library();
        let mix = EndmemberMix::new(vec![(Endmember::Sand, 0.5), (Endmember::WaterIce, 0.5)]);
        assert_eq!(mix.reflectance_at(&l, 550.0), None);
        assert_eq!(mix.band_mean(&l, 400.0, 700.0), None);
    }

    #[test]
    fn partial_weights_are_not_renormalised() {
        let l = library();
        let mix = EndmemberMix::new(vec![(Endmember::Sand, 0.25)]);
        assert_eq!(mix.total_weight(), 0.25);
        let direct = l.get(Endmember::Sand).unwrap().at_nm(600.0).unwrap();
        assert!((mix.reflectance_at(&l, 600.0).unwrap() - 0.25 * direct).abs() < 1e-12);
    }

    #[test]
    fn parse_rejects_unknown_endmembers_and_short_rows() {
        let bad = "endmember,wavelength_nm,reflectance\nUnobtainium,500,0.5\n";
        assert!(ReflectanceLibrary::parse(bad)
            .unwrap_err()
            .to_string()
            .contains("unknown endmember"));
        let short = "endmember,wavelength_nm,reflectance\nSand,500\n";
        assert!(ReflectanceLibrary::parse(short)
            .unwrap_err()
            .to_string()
            .contains("expected 3 columns"));
    }

    #[test]
    fn parse_rejects_an_empty_table() {
        assert!(ReflectanceLibrary::parse("# nothing here\n").is_err());
    }
}
