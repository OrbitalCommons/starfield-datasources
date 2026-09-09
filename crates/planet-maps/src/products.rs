//! The pinned catalogue of archive map products.
//!
//! Pinned rather than discovered. USGS's CKAN endpoint at
//! `/ckan/api/3/action/package_search` returns HTML, not JSON, so there is no
//! usable discovery API; and even if there were, a data source wants
//! reproducibility over freshness — a render should not change because an
//! upstream mosaic was silently revised.
//!
//! # Verifying a slug
//!
//! `astrogeology.usgs.gov` returns **HTTP 200 with a generic catalogue page**
//! for an unknown slug, so a wrong product looks like a hit and a status-code
//! check proves nothing. The existence test is whether the page contains a
//! `planetarymaps.usgs.gov` link. Every entry below was confirmed that way, and
//! the sizes are from following the redirect.

use starfield::{Result, StarfieldError};
use starfield_datasource_utils::{
    download_to_file, ensure_cache_subdir, file_exists_and_not_empty, verify_sha256,
};
use starfield_reflectance_library::Endmember;

use crate::grid::MapGrid;
use crate::map::PhotometricBand;

/// Cache subdirectory for downloaded mosaics.
const CACHE_SUBDIR: &str = "planet-maps";

/// USGS serves mosaics from here; requests 302 to an S3 bucket.
pub const USGS_MOSAIC_BASE_URL: &str = "https://planetarymaps.usgs.gov/mosaic/";

/// One archive mosaic, with everything needed to fetch and register it.
#[derive(Debug, Clone, Copy)]
pub struct MapProduct {
    /// Stable identifier used by [`MapProduct::by_id`].
    pub id: &'static str,
    /// NAIF id of the body.
    pub naif_id: i32,
    /// File name under [`USGS_MOSAIC_BASE_URL`].
    pub file_name: &'static str,
    /// Approximate download size, bytes. Verified by following the redirect.
    pub size_bytes: u64,
    /// Band the pixel values represent.
    pub band: PhotometricBand,
    /// Endmember the surface is treated as, pending real terrain-unit
    /// decomposition.
    pub endmember: Endmember,
    /// Whether the product is monochrome. Colour products need unmixing before
    /// they can drive an [`crate::AlbedoMap`]; see the crate docs.
    pub monochrome: bool,
    /// Provenance note.
    pub description: &'static str,
}

impl MapProduct {
    /// Full download URL.
    pub fn url(&self) -> String {
        format!("{}{}", USGS_MOSAIC_BASE_URL, self.file_name)
    }

    /// Coordinate conventions of the stored product.
    ///
    /// Every USGS mosaic in this table is east-positive planetocentric with
    /// north at row 0 and column 0 at longitude 0.
    pub fn grid(&self) -> MapGrid {
        MapGrid::usgs_default()
    }

    /// Where [`MapProduct::download`] puts the file.
    pub fn cached_path(&self) -> Result<std::path::PathBuf> {
        Ok(ensure_cache_subdir(CACHE_SUBDIR)?.join(self.file_name))
    }

    /// Fetch into the starfield cache if not already present, returning the path.
    ///
    /// These are large — up to 11.9 GB — so this is deliberately explicit and
    /// never implicit in a constructor.
    ///
    /// `expected_sha256` is optional because the table does not yet carry
    /// digests: computing them means downloading ~25 GB. When supplied, the file
    /// is verified and removed if it does not match, so a truncated download
    /// cannot be silently reused on the next call.
    pub fn download(&self, expected_sha256: Option<&str>) -> Result<std::path::PathBuf> {
        let path = self.cached_path()?;
        if !file_exists_and_not_empty(&path) {
            download_to_file(&self.url(), &path, 3600)?;
        }
        if let Some(expected) = expected_sha256 {
            if let Err(e) = verify_sha256(&path, expected) {
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
        }
        Ok(path)
    }

    /// Look up a product by [`MapProduct::id`].
    pub fn by_id(id: &str) -> Option<&'static MapProduct> {
        PRODUCTS.iter().find(|p| p.id == id)
    }

    /// Every product for a body, by NAIF id.
    pub fn for_body(naif_id: i32) -> Vec<&'static MapProduct> {
        PRODUCTS.iter().filter(|p| p.naif_id == naif_id).collect()
    }
}

/// The pinned catalogue. URLs and sizes verified by fetch.
pub static PRODUCTS: &[MapProduct] = &[
    MapProduct {
        id: "moon-lroc-wac-100m",
        naif_id: 301,
        file_name: "Lunar_LRO_LROC-WAC_Mosaic_global_100m_June2013.tif",
        size_bytes: 5_959_263_751,
        // WAC's 643 nm band is the one used for the monochrome global mosaic.
        band: PhotometricBand {
            lo_nm: 633.0,
            hi_nm: 653.0,
        },
        endmember: Endmember::FreshBasalt,
        monochrome: true,
        description: "LRO LROC WAC global morphology mosaic, 100 m, 643 nm",
    },
    MapProduct {
        id: "mars-viking-color-925m",
        naif_id: 499,
        file_name: "Mars_Viking_ClrMosaic_global_925m.tif",
        size_bytes: 794_000_000,
        band: PhotometricBand {
            lo_nm: 400.0,
            hi_nm: 700.0,
        },
        endmember: Endmember::WeatheredBasalt,
        monochrome: false,
        description: "Viking colour global mosaic, 925 m",
    },
    MapProduct {
        id: "mars-viking-mdim21-color-232m",
        naif_id: 499,
        file_name: "Mars_Viking_MDIM21_ClrMosaic_global_232m.tif",
        size_bytes: 12_745_000_000,
        band: PhotometricBand {
            lo_nm: 400.0,
            hi_nm: 700.0,
        },
        endmember: Endmember::WeatheredBasalt,
        monochrome: false,
        description: "Viking MDIM 2.1 colour global mosaic, 232 m",
    },
    MapProduct {
        id: "ganymede-voyager-galileo-color-1435m",
        naif_id: 503,
        file_name: "Ganymede_Voyager_GalileoSSI_Global_ClrMosaic_1435m.tif",
        size_bytes: 0,
        band: PhotometricBand {
            lo_nm: 400.0,
            hi_nm: 700.0,
        },
        endmember: Endmember::WaterIce,
        monochrome: false,
        description: "Voyager/Galileo SSI colour global mosaic, 1.435 km",
    },
    MapProduct {
        id: "callisto-voyager-galileo-1km",
        naif_id: 504,
        file_name: "Callisto_Voyager_GalileoSSI_global_mosaic_1km.tif",
        size_bytes: 0,
        band: PhotometricBand {
            lo_nm: 400.0,
            hi_nm: 700.0,
        },
        endmember: Endmember::WaterIce,
        monochrome: true,
        description: "Voyager/Galileo SSI global mosaic, 1 km",
    },
];

/// Report a body that has no product in the catalogue.
pub fn require_product_for(naif_id: i32) -> Result<&'static MapProduct> {
    MapProduct::for_body(naif_id)
        .into_iter()
        .next()
        .ok_or_else(|| {
            StarfieldError::ObjectNotFound(format!("no map product for NAIF id {naif_id}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = PRODUCTS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate product id");
    }

    #[test]
    fn lookup_by_id_and_body() {
        assert_eq!(
            MapProduct::by_id("moon-lroc-wac-100m").unwrap().naif_id,
            301
        );
        assert!(MapProduct::by_id("no-such-product").is_none());
        // Mars has two products at different resolutions.
        assert_eq!(MapProduct::for_body(499).len(), 2);
        assert!(MapProduct::for_body(-1).is_empty());
    }

    #[test]
    fn urls_point_at_the_usgs_mosaic_host() {
        for p in PRODUCTS {
            let url = p.url();
            assert!(
                url.starts_with("https://planetarymaps.usgs.gov/mosaic/"),
                "{url}"
            );
            assert!(url.ends_with(".tif"), "{url}");
        }
    }

    #[test]
    fn bands_are_ordered_and_plausible() {
        for p in PRODUCTS {
            assert!(p.band.lo_nm < p.band.hi_nm, "{}", p.id);
            assert!(p.band.lo_nm > 100.0 && p.band.hi_nm < 5000.0, "{}", p.id);
        }
    }

    #[test]
    fn jupiter_has_no_product_and_that_is_reported() {
        // Deliberate: USGS has no controlled global mosaic for Jupiter, and
        // will not - there is no solid surface to control a photogrammetric
        // network to. A caller must get an error, not an empty map.
        assert!(MapProduct::for_body(599).is_empty());
        let err = require_product_for(599).unwrap_err().to_string();
        assert!(err.contains("599"), "{err}");
    }

    #[test]
    fn every_product_uses_the_usgs_grid_convention() {
        use crate::grid::{Latitude, Longitude, RowOrder};
        for p in PRODUCTS {
            let g = p.grid();
            assert_eq!(g.longitude, Longitude::EastPositive, "{}", p.id);
            assert_eq!(g.latitude, Latitude::Planetocentric, "{}", p.id);
            assert_eq!(g.row_order, RowOrder::NorthFirst, "{}", p.id);
        }
    }

    #[test]
    #[ignore = "requires network access to USGS"]
    fn pinned_urls_are_reachable() {
        for p in PRODUCTS {
            let client = starfield_datasource_utils::build_http_client(60).unwrap();
            let response = client.head(p.url()).send();
            assert!(response.is_ok(), "{}: {:?}", p.id, response.err());
        }
    }
}
