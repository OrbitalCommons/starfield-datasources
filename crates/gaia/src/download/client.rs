//! Release-parameterized downloader, MD5 verifier, and local cache lister.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use regex::Regex;
use starfield::{Result, StarfieldError};
use starfield_datasource_utils::artifact::materialize;
use starfield_datasource_utils::datastore::{
    Artifact, ArtifactKey, ContentCheck, Datastore, DatastoreBuilder, Source,
};
use starfield_datasource_utils::{
    adopt_legacy, build_http_client, cache_dir, check_response_status, datastore_error,
};

use crate::common::traits::GaiaRelease;

/// Entry point for fetching and caching CSV.gz files for a specific Gaia release.
pub struct Downloader<R: GaiaRelease> {
    _marker: PhantomData<R>,
}

impl<R: GaiaRelease> Downloader<R> {
    /// Named immutable release artifact, shared with the mirror manifest.
    pub fn artifact(filename: &str) -> Result<Artifact> {
        if filename.is_empty()
            || filename.contains(['/', '\\'])
            || filename == "."
            || filename == ".."
        {
            return Err(StarfieldError::DataError(
                "Gaia filename must be a bare filename".into(),
            ));
        }
        let check = if filename.ends_with(".gz") {
            ContentCheck::All(vec![
                ContentCheck::default_binary(),
                ContentCheck::magic(vec![vec![0x1f, 0x8b]], false),
            ])
        } else {
            ContentCheck::All(vec![ContentCheck::NotHtml, ContentCheck::MinBytes(1)])
        };
        let mut artifact = Artifact::new(
            ArtifactKey::new(format!(
                "gaia/{}/gaia_source/{filename}",
                R::RELEASE.as_str().to_ascii_lowercase()
            ))
            .map_err(datastore_error)?,
            vec![Source::new(format!("{}{filename}", R::BASE_URL))],
        )
        .with_check(check);
        artifact.provenance.description = format!("ESA Gaia {} {filename}", R::RELEASE.as_str());
        artifact.provenance.license =
            "ESA Gaia archive data; acknowledge Gaia and DPAC under the release citation policy"
                .into();
        Ok(artifact)
    }
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
        let store = Datastore::from_env().map_err(datastore_error)?;
        adopt_legacy(&store, &Self::artifact(R::MD5_FILENAME)?, &md5_path)?;
        Self::checksums_with(&store)
    }

    /// Read release checksums using only the supplied store.
    pub fn checksums_with(store: &Datastore) -> Result<HashMap<String, String>> {
        let path = store
            .get(&Self::artifact(R::MD5_FILENAME)?)
            .map_err(datastore_error)?;
        let file = File::open(path)?;
        let mut map = HashMap::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(StarfieldError::IoError)?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2
                && parts[0].len() == 32
                && parts[0].bytes().all(|b| b.is_ascii_hexdigit())
            {
                let checksum = parts[0].to_ascii_lowercase();
                let name = parts[1].trim_start_matches('*').to_string();
                map.insert(name, checksum);
            }
        }
        Ok(map)
    }

    /// Download a single CSV.gz file, verify its MD5, return the cached path.
    pub fn download_file(filename: &str) -> Result<PathBuf> {
        let artifact = Self::artifact(filename)?;
        let dir = Self::ensure_cache_dir()?;
        let dest = dir.join(filename);
        let store = Datastore::from_env().map_err(datastore_error)?;
        adopt_legacy(&store, &artifact, &dest)?;
        adopt_legacy(
            &store,
            &Self::artifact(R::MD5_FILENAME)?,
            &dir.join(R::MD5_FILENAME),
        )?;
        let blob = Self::download_file_with(&store, filename)?;
        materialize(&blob, &dest)?;
        Ok(dest)
    }

    /// Fetch and verify using only the supplied store; no ambient legacy files.
    pub fn download_file_with(store: &Datastore, filename: &str) -> Result<PathBuf> {
        let artifact = Self::artifact(filename)?;
        let checksums = Self::checksums_with(store)?;
        let expected = checksums.get(filename).ok_or_else(|| {
            StarfieldError::DataError(format!(
                "{filename} is absent from the Gaia release checksum manifest"
            ))
        })?;
        let dest = store.get(&artifact).map_err(datastore_error)?;
        let actual = md5_hex(&dest)?;
        if actual != *expected {
            store.remove(&artifact.key).map_err(datastore_error)?;
            return Err(StarfieldError::DataError(format!(
                "md5 mismatch for {}: expected {}, got {}",
                filename, expected, actual
            )));
        }
        Ok(dest)
    }

    /// Open a read of a validated catalog file. The complete download is checked
    /// in an isolated temporary store before reading; disk use is bounded by one
    /// file and reclaimed when the reader is dropped. Memory does not grow with
    /// the artifact. Use with [`CsvSourceReader::from_reader`](crate::common::reader::CsvSourceReader::from_reader).
    /// Caller is
    /// responsible for setting `is_gz=true` since CSV.gz is the only on-disk
    /// format Gaia publishes for `gaia_source`.
    pub fn stream_file(filename: &str) -> Result<Box<dyn Read + Send>> {
        // Keep multi-GB scratch on the configured cache filesystem, not TMPDIR.
        let root_store = Datastore::from_env().map_err(datastore_error)?;
        let scratch = tempfile::tempdir_in(root_store.cache_root())?;
        let store = DatastoreBuilder::from_env()
            .map_err(datastore_error)?
            .cache_root(scratch.path().to_path_buf())
            .build()
            .map_err(datastore_error)?;
        let path = Self::download_file_with(&store, filename)?;
        Ok(Box::new(TemporaryCatalog {
            file: File::open(path)?,
            _directory: scratch,
        }))
    }

    /// Explicitly evict a raw file and its filename alias after excerpting.
    pub fn remove_cached(filename: &str) -> Result<()> {
        let artifact = Self::artifact(filename)?;
        Datastore::from_env()
            .map_err(datastore_error)?
            .remove(&artifact.key)
            .map_err(datastore_error)?;
        let path = Self::cache_dir().join(filename);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
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

struct TemporaryCatalog {
    file: File,
    _directory: tempfile::TempDir,
}

impl Read for TemporaryCatalog {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(bytes)
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
