//! Download the NSA catalog from the SDSS Science Archive Server.

use std::path::PathBuf;

use starfield::{Result, StarfieldError};
use starfield_datasource_utils::{
    cache_dir, download_to_file, ensure_cache_subdir, file_exists_and_not_empty,
};

/// SDSS DR17 hosts the v1.0.1 NSA at this URL. ~3 GB.
pub const NSA_URL: &str = "https://data.sdss.org/sas/dr17/manga/atlas/v1_0_1/nsa_v1_0_1.fits";

const NSA_FILENAME: &str = "nsa_v1_0_1.fits";

/// Cache subdirectory: `~/.cache/starfield/nsa/`.
pub fn nsa_cache_dir() -> PathBuf {
    cache_dir().join("nsa")
}

fn ensure_nsa_dir() -> Result<PathBuf> {
    ensure_cache_subdir("nsa").map_err(StarfieldError::IoError)
}

/// Download the NSA catalog FITS file (~3 GB) into the per-user cache, or
/// return the cached path if already present and non-empty.
///
/// The download is **not** MD5-verified: SDSS doesn't publish a stable
/// per-file checksum file for atlas releases. The atomic-`.tmp`+rename
/// behavior in `download_to_file` ensures partial downloads aren't mistaken
/// for complete ones.
pub fn download_nsa() -> Result<PathBuf> {
    let dir = ensure_nsa_dir()?;
    let dest = dir.join(NSA_FILENAME);
    if file_exists_and_not_empty(&dest) {
        return Ok(dest);
    }
    eprintln!(
        "downloading NSA catalog (~3 GB) from {} to {} ...",
        NSA_URL,
        dest.display()
    );
    download_to_file(NSA_URL, &dest, 3600)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_under_starfield() {
        let p = nsa_cache_dir();
        assert!(p.to_string_lossy().contains(".cache/starfield/nsa"));
    }
}
