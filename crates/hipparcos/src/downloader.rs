//! Downloader module for retrieving astronomical data
//!
//! This module handles downloading and caching of astronomical data files.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use starfield::Result;
use starfield::StarfieldError;
use starfield_datasource_utils::{download_to_file, ensure_cache_dir, file_exists_and_not_empty};

// Hipparcos catalog URL
const HIPPARCOS_URL: &str = "https://cdsarc.cds.unistra.fr/ftp/cats/I/239/hip_main.dat";

/// Base URL for JPL planetary ephemeris BSP files
const JPL_BSP_URL: &str = "https://ssd.jpl.nasa.gov/ftp/eph/planets/bsp/";

/// Base URL for NAIF satellite SPK files
const NAIF_SATELLITES_URL: &str =
    "https://naif.jpl.nasa.gov/pub/naif/generic_kernels/spk/satellites/";

/// Get the cache directory path
pub fn get_cache_dir() -> PathBuf {
    starfield_datasource_utils::cache_dir()
}

/// Decompress a gzipped file
/// Currently unused as we're using synthetic data, but kept for future reference
#[allow(dead_code)]
fn decompress_gzip<P: AsRef<Path>, Q: AsRef<Path>>(gz_path: P, output_path: Q) -> Result<()> {
    let file = File::open(&gz_path).map_err(StarfieldError::IoError)?;

    // Check if file is a valid gzip file (gzip header starts with magic numbers 0x1F 0x8B)
    let mut header = [0u8; 2];
    {
        let mut file_clone = file.try_clone().map_err(StarfieldError::IoError)?;
        if file_clone.read_exact(&mut header).is_err() || header != [0x1F, 0x8B] {
            return Err(StarfieldError::DataError(format!(
                "Invalid gzip file: {:?} is not a valid gzip header",
                header
            )));
        }
    }

    let gz = BufReader::new(file);
    let mut decoder = flate2::read::GzDecoder::new(gz);

    // Try to validate the gzip file by reading a bit
    let mut test_buffer = [0u8; 1024];
    if decoder.read(&mut test_buffer).is_err() {
        // If we get an error, the file might be corrupted
        // Remove the file and return an error
        let _ = fs::remove_file(&gz_path);
        return Err(StarfieldError::DataError(
            "Downloaded file appears to be corrupt. File removed, please try again.".to_string(),
        ));
    }

    // Reset the decoder and actually decompress
    let file = File::open(gz_path).map_err(StarfieldError::IoError)?;
    let gz = BufReader::new(file);
    let mut decoder = flate2::read::GzDecoder::new(gz);

    let output_file = File::create(&output_path).map_err(StarfieldError::IoError)?;
    let mut writer = BufWriter::new(output_file);

    match io::copy(&mut decoder, &mut writer) {
        Ok(_) => {
            writer.flush().map_err(StarfieldError::IoError)?;
            Ok(())
        }
        Err(e) => {
            // Clean up partial files on error
            let _ = fs::remove_file(&output_path);
            Err(StarfieldError::DataError(format!(
                "Failed to decompress file: {}",
                e
            )))
        }
    }
}

/// Resolve the download URL for a known data filename.
///
/// Returns `Some(full_url)` if the filename matches a known pattern,
/// `None` otherwise. Full URLs (containing `://`) pass through unchanged.
pub fn resolve_url(filename: &str) -> Option<String> {
    if filename.contains("://") {
        return Some(filename.to_string());
    }

    if filename.ends_with(".bsp") {
        let base = if filename.starts_with("jup") {
            NAIF_SATELLITES_URL
        } else {
            JPL_BSP_URL
        };
        return Some(format!("{}{}", base, filename));
    }

    None
}

/// Ensure a data file is available locally, downloading it if necessary.
///
/// Checks `data_dir` (or the default cache `~/.cache/starfield/`) for the file.
/// If not found, resolves the URL from the filename and downloads it.
/// Returns the path to the local file.
pub fn download_or_cache(filename: &str, data_dir: Option<&Path>) -> Result<PathBuf> {
    let dir = match data_dir {
        Some(d) => {
            fs::create_dir_all(d).map_err(StarfieldError::IoError)?;
            d.to_path_buf()
        }
        None => ensure_cache_dir().map_err(StarfieldError::IoError)?,
    };

    let local_path = dir.join(filename);

    if file_exists_and_not_empty(&local_path) {
        return Ok(local_path);
    }

    let url = resolve_url(filename).ok_or_else(|| {
        StarfieldError::DataError(format!(
            "Unknown file '{}'. Provide a recognized filename (e.g. de421.bsp) or a full URL.",
            filename
        ))
    })?;

    eprintln!("Downloading {} ...", url);
    download_to_file(&url, &local_path, 600)?;
    eprintln!("Saved to {}", local_path.display());

    Ok(local_path)
}

/// Download the Hipparcos catalog
pub fn download_hipparcos() -> Result<PathBuf> {
    let cache_dir = ensure_cache_dir().map_err(StarfieldError::IoError)?;

    // File paths
    let dat_path = cache_dir.join("hip_main.dat");

    // If the file already exists and is not empty, return its path
    if file_exists_and_not_empty(&dat_path) {
        println!("Using cached Hipparcos catalog from {}", dat_path.display());
        return Ok(dat_path);
    }

    // Check if hip_main.dat exists in the project root (for CI environments)
    let project_root_dat = PathBuf::from("hip_main.dat");
    if file_exists_and_not_empty(&project_root_dat) {
        println!(
            "Using Hipparcos catalog from project root: {}",
            project_root_dat.display()
        );

        // Copy the file to the cache directory
        fs::copy(&project_root_dat, &dat_path).map_err(StarfieldError::IoError)?;
        println!("Copied Hipparcos catalog to cache: {}", dat_path.display());
        return Ok(dat_path);
    }

    // Download the real Hipparcos catalog
    println!("Downloading Hipparcos catalog from {}...", HIPPARCOS_URL);
    println!("This may take a moment as the catalog is approximately 36MB");

    // Attempt to download the file
    match download_to_file(HIPPARCOS_URL, &dat_path, 30) {
        Ok(_) => {
            println!(
                "Hipparcos catalog downloaded successfully to {}",
                dat_path.display()
            );
            Ok(dat_path)
        }
        Err(e) => {
            // If download fails, we could provide a fallback to synthetic data, but
            // for now we'll just return the error
            println!("Failed to download Hipparcos catalog: {}", e);
            println!("Check your internet connection or try again later.");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starfield_datasource_utils::assert_endpoint_reachable;

    #[test]
    fn test_cache_dir() {
        let cache_dir = get_cache_dir();
        assert!(cache_dir.to_str().unwrap().contains(".cache/starfield"));
    }

    #[test]
    fn test_resolve_url_bsp() {
        assert_eq!(
            resolve_url("de421.bsp"),
            Some("https://ssd.jpl.nasa.gov/ftp/eph/planets/bsp/de421.bsp".to_string())
        );
    }

    #[test]
    fn test_resolve_url_jupiter_bsp() {
        assert_eq!(
            resolve_url("jup365.bsp"),
            Some(
                "https://naif.jpl.nasa.gov/pub/naif/generic_kernels/spk/satellites/jup365.bsp"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_resolve_url_full_url_passthrough() {
        let url = "https://example.com/custom.bsp";
        assert_eq!(resolve_url(url), Some(url.to_string()));
    }

    #[test]
    fn test_resolve_url_unknown() {
        assert_eq!(resolve_url("unknown.xyz"), None);
    }

    #[test]
    fn test_download_or_cache_cached_file() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.bsp");
        std::fs::write(&test_file, b"fake bsp data").unwrap();

        let result = download_or_cache("test.bsp", Some(dir.path()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file);
    }

    #[test]
    fn test_download_or_cache_unknown_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = download_or_cache("unknown.xyz", Some(dir.path()));
        assert!(result.is_err());
    }

    /// Verify all known download endpoints respond to HEAD requests.
    ///
    /// This catches broken URLs in CI without streaming large files.
    #[test]
    fn test_known_endpoints_reachable() {
        let filenames = [
            "de421.bsp",
            "de405.bsp",
            "de430t.bsp",
            "de440.bsp",
            "de441.bsp",
            "jup365.bsp",
        ];

        for filename in filenames {
            let url = resolve_url(filename).expect("resolve_url returned None");
            assert_endpoint_reachable(&url);
        }
    }
}
