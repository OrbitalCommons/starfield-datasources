//! Release-parameterized downloader, MD5 verifier, and local cache lister.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use regex::Regex;
use starfield::{Result, StarfieldError};
use starfield_datasource_utils::{
    build_http_client, cache_dir, check_response_status, download_to_file,
    file_exists_and_not_empty,
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
    /// ESA's user-facing URL (`cdn.gea.esac.esa.int`) is just a JS-rendered
    /// file browser shell, so we paginate the underlying CDN77 S3-style XML
    /// listing instead. Returns filenames matching `R::FILE_REGEX`, sorted
    /// and deduplicated.
    pub fn list_remote() -> Result<Vec<String>> {
        const CDN_HOST: &str = "https://cdn.gea.esac.esa.int/";
        const INDEX_HOST: &str = "https://gaia.eu-1.cdn77-storage.com/";
        let prefix = R::BASE_URL
            .strip_prefix(CDN_HOST)
            .ok_or_else(|| {
                StarfieldError::DataError(format!(
                    "BASE_URL {} doesn't start with the expected CDN host {}",
                    R::BASE_URL,
                    CDN_HOST
                ))
            })?
            .to_string();

        let client = build_http_client(60)?;
        let re = Regex::new(R::FILE_REGEX).map_err(|e| {
            StarfieldError::DataError(format!(
                "compile file regex for {}: {}",
                R::RELEASE.as_str(),
                e
            ))
        })?;

        let mut all = std::collections::BTreeSet::new();
        let mut marker = String::new();
        for _page in 0..50 {
            let url = if marker.is_empty() {
                format!("{}?prefix={}&delimiter=/", INDEX_HOST, prefix)
            } else {
                format!(
                    "{}?prefix={}&delimiter=/&marker={}",
                    INDEX_HOST,
                    prefix,
                    urlencode(&marker),
                )
            };
            let resp = check_response_status(
                client.get(&url).send().map_err(|e| {
                    StarfieldError::DataError(format!("fetch {} index: {}", R::RELEASE.as_str(), e))
                })?,
                &format!("Gaia {} index page", R::RELEASE.as_str()),
            )?;
            let body = resp
                .text()
                .map_err(|e| StarfieldError::DataError(format!("read index body: {}", e)))?;

            let mut last_key = String::new();
            for cap in re.captures_iter(&body) {
                if let Some(m) = cap.get(1) {
                    let name = m.as_str().to_string();
                    last_key = format!("{}{}", prefix, name);
                    all.insert(name);
                }
            }
            let truncated = body.contains("<IsTruncated>true</IsTruncated>");
            if !truncated || last_key.is_empty() {
                break;
            }
            marker = last_key;
        }

        if all.is_empty() {
            return Err(StarfieldError::DataError(format!(
                "no files matched FILE_REGEX for {} at {} (prefix {})",
                R::RELEASE.as_str(),
                INDEX_HOST,
                prefix,
            )));
        }
        Ok(all.into_iter().collect())
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

    /// Open a streaming HTTP read of a catalog file — returns a `Box<dyn Read>`
    /// over the gzipped bytes. Use with [`CsvSourceReader::from_reader`](crate::common::reader::CsvSourceReader::from_reader)
    /// to excerpt without ever writing the raw file to disk. Caller is
    /// responsible for setting `is_gz=true` since CSV.gz is the only on-disk
    /// format Gaia publishes for `gaia_source`.
    pub fn stream_file(filename: &str) -> Result<Box<dyn Read + Send>> {
        let url = format!("{}{}", R::BASE_URL, filename);
        let client = build_http_client(600)?;
        let resp = check_response_status(
            client
                .get(&url)
                .send()
                .map_err(|e| StarfieldError::DataError(format!("HTTP get {}: {}", url, e)))?,
            &url,
        )?;
        Ok(Box::new(resp))
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

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
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
