//! Per-observation data-product listing + FITS download.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use starfield::{Result, StarfieldError};
use starfield_datasource_utils::{download_to_file, file_exists_and_not_empty};

use crate::client::MastClient;

/// One artifact produced by an observation. CAOM groups artifacts by
/// `productType` (`SCIENCE`, `PREVIEW`, `INFO`, `AUXILIARY`,
/// `THUMBNAIL`); for HST the headline science product is typically a
/// `_drz.fits` (drizzled mosaic) or `_flt.fits` (flatfielded per-
/// exposure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProduct {
    /// Bare filename, e.g. `j8c0a1011_drz.fits`.
    #[serde(rename = "productFilename")]
    pub filename: String,

    /// CAOM productType. Use [`Self::product_type`] for the parsed
    /// enum.
    #[serde(rename = "productType", default)]
    pub product_type: Option<String>,

    /// MIME type / file class string (`image/fits`, etc.).
    #[serde(rename = "productSubGroupDescription", default)]
    pub product_subgroup: Option<String>,

    /// Public download URL. Often a `mast:` URI; we resolve it via
    /// [`MastClient::download_product`].
    #[serde(rename = "dataURI", default)]
    pub data_uri: Option<String>,

    /// Direct HTTPS download URL (when MAST publishes one). Preferred
    /// over `data_uri` when present.
    #[serde(rename = "dataURL", default)]
    pub data_url: Option<String>,

    /// File size in bytes (when MAST reports it).
    #[serde(rename = "size", default)]
    pub size_bytes: Option<u64>,

    /// Parent observation id this product belongs to.
    #[serde(rename = "obs_id", default)]
    pub obs_id: Option<String>,

    /// Description text MAST attaches to the product (rarely useful).
    #[serde(rename = "description", default)]
    pub description: Option<String>,

    /// Anything else MAST returned that we didn't model above.
    #[serde(flatten)]
    pub extras: BTreeMap<String, JsonValue>,
}

/// Parsed CAOM productType. Use [`DataProduct::product_type`] to get
/// the enum form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductType {
    Science,
    Preview,
    Info,
    Auxiliary,
    Thumbnail,
    Other,
}

impl ProductType {
    /// Parse a CAOM `productType` string. Unknown values map to
    /// [`ProductType::Other`]. Named `parse_caom` (rather than `from_str`)
    /// to avoid shadowing the `std::str::FromStr` trait — implementing
    /// the trait would force a `Result` return that no caller wants.
    pub fn parse_caom(s: &str) -> Self {
        match s {
            "SCIENCE" => Self::Science,
            "PREVIEW" => Self::Preview,
            "INFO" => Self::Info,
            "AUXILIARY" => Self::Auxiliary,
            "THUMBNAIL" => Self::Thumbnail,
            _ => Self::Other,
        }
    }
}

impl DataProduct {
    pub fn product_type_enum(&self) -> ProductType {
        ProductType::parse_caom(self.product_type.as_deref().unwrap_or(""))
    }

    /// True for `productType=SCIENCE` *and* a `.fits` filename.
    pub fn is_science_fits(&self) -> bool {
        self.product_type_enum() == ProductType::Science
            && self.filename.to_ascii_lowercase().ends_with(".fits")
    }

    /// Resolve the best available download URL: prefer the explicit
    /// `dataURL`, then translate `mast:` URIs to the standard MAST
    /// download endpoint.
    pub fn resolve_url(&self) -> Option<String> {
        if let Some(url) = self.data_url.as_ref() {
            return Some(url.clone());
        }
        if let Some(uri) = self.data_uri.as_ref() {
            if let Some(rest) = uri.strip_prefix("mast:") {
                return Some(format!(
                    "https://mast.stsci.edu/api/v0.1/Download/file?uri=mast:{}",
                    rest
                ));
            }
            return Some(uri.clone());
        }
        None
    }
}

impl MastClient {
    /// List the data products for a given observation. Backed by the
    /// `Mast.Caom.Products` Mashup service.
    ///
    /// `obsid` is the **internal numeric** id (the `obsid` column in
    /// CAOM, not the human-readable `obs_id`). Use
    /// [`CaomObservation::obsid_string`] to extract it from a row, or
    /// pass the numeric value directly.
    pub fn data_products(&self, obsid: &str) -> Result<Vec<DataProduct>> {
        let params = json!({ "obsid": obsid });
        let response = self.invoke::<DataProduct>("Mast.Caom.Products", params)?;
        Ok(response.data)
    }

    /// Download `product` to the per-crate cache (`~/.cache/starfield/mast/`).
    /// If the file already exists at the cache path and is non-empty,
    /// returns the path without re-fetching.
    ///
    /// Errors out when the product has no resolvable download URL.
    pub fn download_product(&self, product: &DataProduct) -> Result<PathBuf> {
        let url = product.resolve_url().ok_or_else(|| {
            StarfieldError::DataError(format!(
                "data product {:?} has neither dataURL nor dataURI",
                product.filename
            ))
        })?;
        let cache = self.cache_dir()?;
        let dest = cache.join(&product.filename);
        if file_exists_and_not_empty(&dest) {
            log::debug!("MAST cache hit for {}", product.filename);
            return Ok(dest);
        }
        log::info!(
            "downloading MAST product {} → {}",
            product.filename,
            dest.display()
        );
        download_to_file(&url, &dest, 60)?;
        Ok(dest)
    }
}
