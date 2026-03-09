//! Shared file download utilities
//!
//! Provides atomic download-to-file with optional progress bar display.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use indicatif::{ProgressBar, ProgressStyle};
use starfield::{Result, StarfieldError};

use crate::{build_http_client, check_response_status};

/// Download `url` to `path` atomically (via a `.tmp` intermediate file).
///
/// Creates parent directories as needed. Shows an `indicatif` progress bar
/// when the server provides a `Content-Length` header.
///
/// `timeout_secs` controls the HTTP client read timeout.
pub fn download_to_file<P: AsRef<Path>>(url: &str, path: P, timeout_secs: u64) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent).map_err(StarfieldError::IoError)?;
    }

    let temp_path = path.as_ref().with_extension("tmp");
    let mut file = BufWriter::new(File::create(&temp_path).map_err(StarfieldError::IoError)?);

    let client = build_http_client(timeout_secs)?;

    let response = check_response_status(
        client
            .get(url)
            .send()
            .map_err(|e| StarfieldError::DataError(format!("Failed to download {}: {}", url, e)))?,
        url,
    )?;

    let total_size = response.content_length().unwrap_or(0);
    let pb = if total_size > 0 {
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40}] {percent}% {bytes}/{total_bytes} ({bytes_per_sec})")
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    let mut reader = std::io::BufReader::new(response);
    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 131_072]; // 128KB chunks

    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| StarfieldError::DataError(format!("Failed to read response: {}", e)))?;

        if n == 0 {
            break;
        }

        file.write_all(&buffer[..n])
            .map_err(StarfieldError::IoError)?;

        downloaded += n as u64;
        if let Some(ref pb) = pb {
            pb.set_position(downloaded);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_and_clear();
    }

    file.flush().map_err(StarfieldError::IoError)?;
    drop(file);

    fs::rename(temp_path, path).map_err(StarfieldError::IoError)?;

    Ok(())
}
