//! Release-parameterized downloader, MD5 verifier, and local cache lister.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use regex::Regex;
use starfield::{Result, StarfieldError};
use starfield_datasource_utils::{
    build_http_client, cache_dir, check_response_status, download_to_file, file_exists_and_not_empty,
};

use crate::common::traits::GaiaRelease;

/// Entry point for fetching and caching CSV.gz files for a specific Gaia release.
pub struct Downloader<R: GaiaRelease> {
    _marker: PhantomData<R>,
}

impl<R: GaiaRelease> Downloader<R> {
    /// Cache directory this release uses, e.g. `~/.cache/starfield/gaia/dr3`.
    pub fn cache_dir() -> PathBuf {
        cache_dir().join(R::CACHE_SUBDIR)
    }

    fn ensure_cache_dir() -> Result<PathBuf> {
        let dir = Self::cache_dir();
        fs::create_dir_all(&dir).map_err(StarfieldError::IoError)?;
        Ok(dir)
    }

    /// List the filenames of every CSV file the remote index exposes.
    ///
    /// Scrapes the HTML directory listing; returns filenames (not URLs).
    pub fn list_remote() -> Result<Vec<String>> {
        let client = build_http_client(60)?;
        let response = check_response_status(
            client.get(R::BASE_URL).send().map_err(|e| {
                StarfieldError::DataError(format!("fetch {} index: {}", R::RELEASE.as_str(), e))
            })?,
            &format!("Gaia {} index", R::RELEASE.as_str()),
        )?;
        let html = response
            .text()
            .map_err(|e| StarfieldError::DataError(format!("read index body: {}", e)))?;

        let re = Regex::new(R::FILE_REGEX).map_err(|e| {
            StarfieldError::DataError(format!("compile file regex for {}: {}", R::RELEASE.as_str(), e))
        })?;
        let files: Vec<String> = re
            .captures_iter(&html)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        if files.is_empty() {
            return Err(StarfieldError::DataError(format!(
                "no files matched FILE_REGEX for {} at {}",
                R::RELEASE.as_str(),
                R::BASE_URL
            )));
        }
        Ok(files)
    }

    /// Cached files on disk for this release.
    pub fn list_cached() -> Result<Vec<PathBuf>> {
        let dir = Self::ensure_cache_dir()?;
        let mut files = Vec::new();
        for entry in fs::read_dir(&dir).map_err(StarfieldError::IoError)? {
            let entry = entry.map_err(StarfieldError::IoError)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let s = path.to_string_lossy();
            if s.ends_with(".csv") || s.ends_with(".csv.gz") {
                files.push(path);
            }
        }
        Ok(files)
    }

    /// Download the release's MD5 checksum file (cached after first fetch).
    pub fn checksums() -> Result<HashMap<String, String>> {
        let dir = Self::ensure_cache_dir()?;
        let md5_path = dir.join(R::MD5_FILENAME);
        if !file_exists_and_not_empty(&md5_path) {
            let url = format!("{}{}", R::BASE_URL, R::MD5_FILENAME);
            download_to_file(&url, &md5_path, 600)?;
        }

        let file = File::open(&md5_path).map_err(StarfieldError::IoError)?;
        let mut map = HashMap::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(StarfieldError::IoError)?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let checksum = parts[0].to_string();
                let name = parts[1].trim_start_matches('*').to_string();
                map.insert(name, checksum);
            }
        }
        Ok(map)
    }

    /// Download a single CSV.gz file, verify its MD5, return the cached path.
    pub fn download_file(filename: &str) -> Result<PathBuf> {
        let dir = Self::ensure_cache_dir()?;
        let dest = dir.join(filename);
        if file_exists_and_not_empty(&dest) {
            return Ok(dest);
        }

        let url = format!("{}{}", R::BASE_URL, filename);
        download_to_file(&url, &dest, 600)?;

        let checksums = Self::checksums()?;
        if let Some(expected) = checksums.get(filename) {
            let actual = md5_hex(&dest)?;
            if actual != *expected {
                return Err(StarfieldError::DataError(format!(
                    "md5 mismatch for {}: expected {}, got {}",
                    filename, expected, actual
                )));
            }
        }
        Ok(dest)
    }

    /// Download every catalog file (optionally up to `max_files`). Failures on
    /// individual files are logged to stderr but do not abort the whole run.
    pub fn download_all(max_files: Option<usize>) -> Result<Vec<PathBuf>> {
        let mut files = Self::list_remote()?;
        if let Some(n) = max_files {
            files.truncate(n);
        }
        let mut out = Vec::new();
        for (i, name) in files.iter().enumerate() {
            eprintln!(
                "[{}/{}] {} {}",
                i + 1,
                files.len(),
                R::RELEASE.as_str(),
                name
            );
            match Self::download_file(name) {
                Ok(p) => out.push(p),
                Err(e) => eprintln!("  skipped ({}): {}", name, e),
            }
        }
        Ok(out)
    }
}

fn md5_hex(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(StarfieldError::IoError)?;
    let mut ctx = md5::Context::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => ctx.consume(&buf[..n]),
            Err(e) => return Err(StarfieldError::IoError(e)),
        }
    }
    Ok(format!("{:x}", ctx.compute()))
}
