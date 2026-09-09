//! Embedded endmember-abundance tiers.
//!
//! A tier stores, per texel, the fractional abundance of each endmember rather
//! than a single albedo — so a renderer gets colour that varies across the disk
//! and not merely brightness.
//!
//! # The file is self-describing
//!
//! The grid geometry lives in the header, not only in the product catalogue. A
//! loader that has never heard of MCD12C1 still cannot misregister the grid by
//! half a cell or mirror it, which is the failure this whole crate is arranged
//! to prevent. The header also carries its provenance, because the fitted class
//! weights are only defensible if you can tell which fit produced a given file.

use std::io::Read;

use starfield::{Result, StarfieldError};
use starfield_reflectance_library::{Endmember, EndmemberMix};

use crate::grid::{Latitude, Longitude, MapGrid, RowOrder};
use crate::map::{SampleFootprint, SurfaceSampler, MU_FLOOR};

/// Format magic. Bump on any layout change.
const MAGIC: &[u8] = b"SFEMv2\n";

/// Where a grid's coordinate bounds sit relative to its cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// Cell *edges* lie on the bounds. A 1440-wide global grid starting at
    /// −180° has its first cell spanning −180.00 to −179.75.
    CellEdge,
    /// Cell *centres* lie on the bounds.
    CellCentre,
}

/// A per-texel endmember abundance grid.
#[derive(Debug, Clone)]
pub struct AbundanceTier {
    width: usize,
    height: usize,
    endmembers: Vec<Endmember>,
    grid: MapGrid,
    registration: Registration,
    naif_id: i32,
    provenance: String,
    /// Mip pyramid. Level 0 is full resolution; each level is plane-major,
    /// `planes[e * w * h + row * w + col]`, 255 = abundance 1.0.
    levels: Vec<TierLevel>,
}

#[derive(Debug, Clone)]
struct TierLevel {
    width: usize,
    height: usize,
    planes: Vec<u8>,
}

impl AbundanceTier {
    /// Parse a gzipped tier.
    pub fn from_gz_bytes(bytes: &[u8]) -> Result<Self> {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut raw)
            .map_err(|e| StarfieldError::DataError(format!("tier: gunzip failed: {e}")))?;
        Self::from_bytes(&raw)
    }

    /// Parse an uncompressed tier.
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        let err = |m: String| StarfieldError::DataError(format!("tier: {m}"));
        if b.len() < MAGIC.len() || &b[..MAGIC.len()] != MAGIC {
            return Err(err(format!(
                "bad magic; expected {:?}",
                String::from_utf8_lossy(MAGIC)
            )));
        }
        let mut p = MAGIC.len();
        let need = |p: usize, n: usize, b: &[u8]| -> Result<()> {
            if p + n > b.len() {
                return Err(err(format!("truncated: wanted {n} bytes at offset {p}")));
            }
            Ok(())
        };

        need(p, 4 + 4 + 8 + 8 + 8, b)?;
        let width = u16::from_le_bytes([b[p], b[p + 1]]) as usize;
        let height = u16::from_le_bytes([b[p + 2], b[p + 3]]) as usize;
        let n_endmembers = b[p + 4] as usize;
        let lon_sense = b[p + 5];
        let lat_kind = b[p + 6];
        let registration = b[p + 7];
        p += 8;
        let naif_id = i32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        p += 4;
        let lon0_deg = f64::from_le_bytes(b[p..p + 8].try_into().unwrap());
        p += 8;
        let lat0_deg = f64::from_le_bytes(b[p..p + 8].try_into().unwrap());
        p += 8;
        let payload_len = u64::from_le_bytes(b[p..p + 8].try_into().unwrap()) as usize;
        p += 8;

        let take_str = |p: &mut usize| -> Result<String> {
            need(*p, 2, b)?;
            let n = u16::from_le_bytes([b[*p], b[*p + 1]]) as usize;
            *p += 2;
            need(*p, n, b)?;
            let s = String::from_utf8_lossy(&b[*p..*p + n]).into_owned();
            *p += n;
            Ok(s)
        };
        let names_blob = take_str(&mut p)?;
        let provenance = take_str(&mut p)?;

        let endmembers: Vec<Endmember> = names_blob
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|s| Endmember::from_id(s).ok_or_else(|| err(format!("unknown endmember {s:?}"))))
            .collect::<Result<_>>()?;
        if endmembers.len() != n_endmembers {
            return Err(err(format!(
                "header says {n_endmembers} endmembers, names list has {}",
                endmembers.len()
            )));
        }

        // Declared length is checked against both the arithmetic and what is
        // actually present, so a truncated download fails here rather than
        // rendering as a dark stripe across the southern hemisphere.
        let expected = n_endmembers * width * height;
        if payload_len != expected {
            return Err(err(format!(
                "header payload_len {payload_len} != {n_endmembers} x {width} x {height} = {expected}"
            )));
        }
        need(p, payload_len, b)?;
        if b.len() - p != payload_len {
            return Err(err(format!(
                "payload is {} bytes, header declares {payload_len}",
                b.len() - p
            )));
        }
        let planes = b[p..p + payload_len].to_vec();

        let grid = MapGrid {
            longitude: match lon_sense {
                0 => Longitude::EastPositive,
                1 => Longitude::WestPositive,
                other => return Err(err(format!("unknown longitude sense {other}"))),
            },
            latitude: match lat_kind {
                0 => Latitude::Planetocentric,
                1 => Latitude::Planetographic,
                other => return Err(err(format!("unknown latitude kind {other}"))),
            },
            lon0_deg,
            // lat0 at the top means row 0 is the northern edge.
            row_order: if lat0_deg > 0.0 {
                RowOrder::NorthFirst
            } else {
                RowOrder::SouthFirst
            },
            flattening: crate::products::EARTH_WGS84_FLATTENING,
        };

        Ok(Self {
            width,
            height,
            endmembers,
            grid,
            registration: match registration {
                0 => Registration::CellEdge,
                1 => Registration::CellCentre,
                other => return Err(err(format!("unknown registration {other}"))),
            },
            naif_id,
            provenance,
            levels: build_pyramid(TierLevel {
                width,
                height,
                planes,
            }),
        })
    }

    /// Grid dimensions, `(width, height)`.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Endmembers, in plane order.
    pub fn endmembers(&self) -> &[Endmember] {
        &self.endmembers
    }

    /// Coordinate conventions, read from the file's own header.
    pub fn grid(&self) -> &MapGrid {
        &self.grid
    }

    /// Whether cell edges or centres lie on the grid bounds.
    pub fn registration(&self) -> Registration {
        self.registration
    }

    /// NAIF id of the body.
    pub fn naif_id(&self) -> i32 {
        self.naif_id
    }

    /// Source archive, class-table version and build-script version.
    pub fn provenance(&self) -> &str {
        &self.provenance
    }

    /// Number of pyramid levels.
    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    /// Abundance of plane `e` at `(col, row)` of `level`, in `[0, 1]`.
    fn plane_at(&self, level: usize, e: usize, col: usize, row: usize) -> f64 {
        let l = &self.levels[level];
        l.planes[e * l.width * l.height + row * l.width + col] as f64 / 255.0
    }

    /// Mix at a texel of a given level.
    fn mix_at(&self, level: usize, u: f64, v: f64) -> EndmemberMix {
        let l = &self.levels[level];
        let col = ((u * l.width as f64) as usize).min(l.width - 1);
        let row = ((v * l.height as f64) as usize).min(l.height - 1);
        let mut weights = Vec::new();
        for (e, &endmember) in self.endmembers.iter().enumerate() {
            let w = self.plane_at(level, e, col, row);
            if w > 0.0 {
                weights.push((endmember, w));
            }
        }
        EndmemberMix::new(weights)
    }

    /// Choose a pyramid level for a sky footprint at emission cosine `mu`.
    ///
    /// Identical policy to [`crate::AlbedoMap::select_level`]: select from the
    /// stretched axis, clamp `mu` at [`MU_FLOOR`], never alias. Sharing the
    /// policy matters — a body with a tier and a body with a scalar map must
    /// not blur differently at the same geometry.
    pub fn select_level(&self, sky_radius_rad: f64, mu: f64) -> Option<SampleFootprint> {
        if !sky_radius_rad.is_finite() || sky_radius_rad <= 0.0 || !mu.is_finite() {
            return None;
        }
        let mu_clamped = mu < MU_FLOOR;
        let mu_eff = mu.max(MU_FLOOR);
        if mu_eff > 1.0 {
            return None;
        }
        let texels_per_rad = self.width as f64 / (2.0 * std::f64::consts::PI);
        let texels = sky_radius_rad / mu_eff * texels_per_rad;
        let level = if texels <= 1.0 {
            0
        } else {
            (texels.log2().floor() as usize).min(self.levels.len() - 1)
        };
        Some(SampleFootprint {
            level,
            texels,
            anisotropy: 1.0 / mu_eff,
            mu_clamped,
        })
    }

    /// Composition at a body-fixed position, full resolution, nearest texel.
    ///
    /// Input is **east-positive planetocentric radians**, as everywhere else in
    /// this crate; the header's conventions are applied internally.
    ///
    /// Endmembers with zero abundance are omitted, so a mix over open ocean has
    /// one entry rather than nine. Use [`AbundanceTier::sample_area`] unless the
    /// footprint is known to be sub-texel.
    pub fn sample(&self, lon_rad: f64, lat_rad: f64) -> Option<EndmemberMix> {
        let (u, v) = self.grid.body_fixed_to_uv(lon_rad, lat_rad)?;
        Some(self.mix_at(0, u, v))
    }

    /// Composition over a footprint, with the resolution level chosen from it.
    ///
    /// The tier equivalent of [`crate::AlbedoMap::sample_area`], and the reason
    /// both implement [`SurfaceSampler`]: a consumer should not branch on
    /// whether a body happens to have real composition data.
    pub fn sample_area(
        &self,
        lon_rad: f64,
        lat_rad: f64,
        sky_radius_rad: f64,
        mu: f64,
    ) -> Option<(EndmemberMix, SampleFootprint)> {
        let (u, v) = self.grid.body_fixed_to_uv(lon_rad, lat_rad)?;
        let footprint = self.select_level(sky_radius_rad, mu)?;
        Some((self.mix_at(footprint.level, u, v), footprint))
    }

    /// Largest plane sum over the grid, in raw u8 units.
    ///
    /// Should not exceed `255 + n_endmembers`: abundances sum to at most 1, and
    /// each plane can round up by at most one unit. A larger value means the
    /// planes do not describe a partition and the file is corrupt.
    pub fn max_plane_sum(&self) -> u32 {
        let l = &self.levels[0];
        let cells = l.width * l.height;
        (0..cells)
            .map(|i| {
                (0..self.endmembers.len())
                    .map(|e| l.planes[e * cells + i] as u32)
                    .sum::<u32>()
            })
            .max()
            .unwrap_or(0)
    }
}

impl SurfaceSampler for AbundanceTier {
    fn sample_area(
        &self,
        lon_rad: f64,
        lat_rad: f64,
        sky_radius_rad: f64,
        mu: f64,
    ) -> Option<(EndmemberMix, SampleFootprint)> {
        AbundanceTier::sample_area(self, lon_rad, lat_rad, sky_radius_rad, mu)
    }

    fn endmembers(&self) -> Vec<Endmember> {
        self.endmembers.clone()
    }
}

/// Box-filter every plane down to a 1x1 level.
///
/// Averaging abundances preserves the partition: if the planes summed to at
/// most 1 before, they do after, so a coarse level is still a valid mix.
fn build_pyramid(base: TierLevel) -> Vec<TierLevel> {
    let mut levels = vec![base];
    while levels.last().unwrap().width > 1 || levels.last().unwrap().height > 1 {
        let prev = levels.last().unwrap();
        let width = (prev.width / 2).max(1);
        let height = (prev.height / 2).max(1);
        let n = prev.planes.len() / (prev.width * prev.height);
        let mut planes = vec![0u8; n * width * height];
        for e in 0..n {
            for row in 0..height {
                for col in 0..width {
                    let mut sum = 0u32;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let sr = (row * 2 + dy).min(prev.height - 1);
                            let sc = (col * 2 + dx).min(prev.width - 1);
                            sum += prev.planes[e * prev.width * prev.height + sr * prev.width + sc]
                                as u32;
                        }
                    }
                    planes[e * width * height + row * width + col] = (sum / 4) as u8;
                }
            }
        }
        levels.push(TierLevel {
            width,
            height,
            planes,
        });
    }
    levels
}

/// The embedded Earth tier: MODIS MCD12G1-derived endmember abundances at 0.25°.
pub const EARTH_TIER_GZ: &[u8] = include_bytes!("../data/earth_endmembers_0p25deg.bin.gz");

/// Parse the embedded Earth tier.
pub fn earth_tier() -> Result<AbundanceTier> {
    AbundanceTier::from_gz_bytes(EARTH_TIER_GZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier() -> AbundanceTier {
        earth_tier().unwrap()
    }

    #[test]
    fn embedded_tier_has_the_expected_shape() {
        let t = tier();
        assert_eq!(t.size(), (1440, 720));
        assert_eq!(t.endmembers().len(), 9);
        assert_eq!(t.naif_id(), 399);
        assert_eq!(t.registration(), Registration::CellEdge);
    }

    #[test]
    fn grid_conventions_come_from_the_file_not_the_catalogue() {
        // The point of the self-describing header: geometry survives without
        // any knowledge of which product this is.
        let t = tier();
        let g = t.grid();
        assert_eq!(g.longitude, Longitude::EastPositive);
        assert_eq!(g.latitude, Latitude::Planetographic);
        assert_eq!(g.lon0_deg, -180.0);
        assert_eq!(g.row_order, RowOrder::NorthFirst);
        assert!(g.flattening > 0.003 && g.flattening < 0.004);
    }

    #[test]
    fn provenance_names_the_granule_and_the_fit() {
        let t = tier();
        let p = t.provenance();
        assert!(p.contains("MCD12C1"), "{p}");
        assert!(p.contains("igbp-fitted-1"), "{p}");
        assert!(p.contains("coastal radius"), "{p}");
    }

    #[test]
    fn plane_sums_never_exceed_a_partition_plus_rounding() {
        // A corrupted plane cannot pass as a plausible mix.
        let t = tier();
        let max = t.max_plane_sum();
        let limit = 255 + t.endmembers().len() as u32;
        assert!(max <= limit, "max plane sum {max} exceeds {limit}");
    }

    #[test]
    fn open_ocean_is_ocean_and_sahara_is_arid() {
        let t = tier();
        // Mid-Pacific, far from any coast.
        let pacific = t.sample((-140f64).to_radians(), 0f64.to_radians()).unwrap();
        let ocean = pacific
            .weights()
            .iter()
            .find(|(e, _)| *e == Endmember::OpenOcean)
            .map(|(_, w)| *w)
            .unwrap_or(0.0);
        assert!(ocean > 0.9, "mid-Pacific ocean fraction {ocean}");

        // Central Sahara.
        let sahara = t.sample(15f64.to_radians(), 23f64.to_radians()).unwrap();
        let arid = sahara
            .weights()
            .iter()
            .find(|(e, _)| *e == Endmember::AridSoil)
            .map(|(_, w)| *w)
            .unwrap_or(0.0);
        assert!(arid > 0.5, "Sahara arid-soil fraction {arid}");
    }

    #[test]
    fn antarctica_is_snow() {
        let t = tier();
        let mix = t.sample(0f64.to_radians(), (-80f64).to_radians()).unwrap();
        let snow = mix
            .weights()
            .iter()
            .find(|(e, _)| *e == Endmember::Snow)
            .map(|(_, w)| *w)
            .unwrap_or(0.0);
        assert!(snow > 0.8, "Antarctic snow fraction {snow}");
    }

    #[test]
    fn closed_canopy_reproduces_the_fitted_split() {
        // Congo basin: uninterrupted evergreen broadleaf, so the mix should be
        // almost exactly the fitted 55/45 forest row.
        //
        // Deliberately not Manaus (-60, -3), which the tier resolves as a third
        // river water and a tenth urban. That is correct - it is a major river
        // confluence with a city of two million - but it makes a poor test of
        // the canopy fit.
        let t = tier();
        let mix = t.sample(20f64.to_radians(), 0f64.to_radians()).unwrap();
        let get = |want: Endmember| {
            mix.weights()
                .iter()
                .find(|(e, _)| *e == want)
                .map(|(_, w)| *w)
                .unwrap_or(0.0)
        };
        let veg = get(Endmember::GreenVegetation);
        let shade = get(Endmember::Shade);
        assert!((veg - 0.55).abs() < 0.03, "vegetation fraction {veg}");
        assert!((shade - 0.45).abs() < 0.03, "shade fraction {shade}");
        assert!(shade < veg);
    }

    #[test]
    fn a_river_city_resolves_as_water_and_urban() {
        // Manaus: the Rio Negro/Amazon confluence with a city of two million.
        // A tier that returned pure forest here would have lost the sub-pixel
        // class fractions that are the whole point of MCD12C1.
        let t = tier();
        let mix = t
            .sample((-60f64).to_radians(), (-3f64).to_radians())
            .unwrap();
        let get = |want: Endmember| {
            mix.weights()
                .iter()
                .find(|(e, _)| *e == want)
                .map(|(_, w)| *w)
                .unwrap_or(0.0)
        };
        assert!(get(Endmember::CoastalWater) > 0.1, "river water fraction");
        assert!(get(Endmember::Asphalt) > 0.02, "urban fraction");
    }

    #[test]
    fn mixes_omit_absent_endmembers() {
        let t = tier();
        let pacific = t.sample((-140f64).to_radians(), 0.0).unwrap();
        assert!(
            pacific.len() < t.endmembers().len(),
            "open ocean should not carry every endmember"
        );
    }

    #[test]
    fn pyramid_halves_down_to_one_texel() {
        let t = tier();
        // 1440x720 -> 11 levels before both axes reach 1.
        assert_eq!(t.levels(), 11);
    }

    #[test]
    fn coarse_levels_stay_valid_mixes() {
        // Averaging abundances preserves the partition, so a blurred texel is
        // still a mix and not something summing past 1.
        let t = tier();
        for level in 0..t.levels() {
            let mix = t.mix_at(level, 0.4, 0.55);
            assert!(
                mix.total_weight() <= 1.02,
                "level {level} total weight {}",
                mix.total_weight()
            );
        }
    }

    #[test]
    fn a_coarser_footprint_selects_a_coarser_level() {
        let t = tier();
        let fine = t.select_level(1e-5, 1.0).unwrap();
        let coarse = t.select_level(1e-2, 1.0).unwrap();
        assert_eq!(fine.level, 0);
        assert!(coarse.level > fine.level, "{coarse:?} vs {fine:?}");
        // And the limb coarsens further, with the clamp reported.
        let limb = t.select_level(1e-2, 1e-9).unwrap();
        assert!(limb.mu_clamped);
        assert!(limb.level >= coarse.level);
        assert!(limb.level < t.levels());
    }

    #[test]
    fn blurring_the_ocean_keeps_it_ocean() {
        // A large footprint in the mid-Pacific must stay water at every level;
        // if the pyramid were built per-texel-max or otherwise wrongly it would
        // drift toward land.
        let t = tier();
        for radius in [1e-5, 1e-3, 1e-2] {
            let (mix, fp) = t
                .sample_area((-140f64).to_radians(), 0.0, radius, 1.0)
                .unwrap();
            let water: f64 = mix
                .weights()
                .iter()
                .filter(|(e, _)| matches!(e, Endmember::OpenOcean | Endmember::CoastalWater))
                .map(|(_, w)| *w)
                .sum();
            assert!(water > 0.85, "level {} water {water}", fp.level);
        }
    }

    #[test]
    fn the_trait_gives_one_interface_for_both_backings() {
        use crate::map::{PhotometricBand, SurfaceSampler};
        use crate::MapGrid;

        let t = tier();
        let m = crate::AlbedoMap::new(
            MapGrid::usgs_default(),
            PhotometricBand::new(400.0, 700.0),
            Endmember::FreshBasalt,
            0.1,
            64,
            32,
            vec![0.05; 64 * 32],
        )
        .unwrap();

        let samplers: Vec<&dyn SurfaceSampler> = vec![&t, &m];
        for s in samplers {
            let (mix, fp) = s.sample_area(0.3, 0.2, 1e-4, 0.8).unwrap();
            assert!(!s.endmembers().is_empty());
            assert!(mix.total_weight() > 0.0);
            assert!(fp.anisotropy > 1.0);
        }
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        assert!(AbundanceTier::from_bytes(b"not a tier").is_err());
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(EARTH_TIER_GZ)
            .read_to_end(&mut raw)
            .unwrap();
        // Chop the payload: the declared length no longer matches.
        let short = &raw[..raw.len() - 1000];
        let err = AbundanceTier::from_bytes(short).unwrap_err().to_string();
        assert!(
            err.contains("payload") || err.contains("truncated"),
            "{err}"
        );
    }
}
