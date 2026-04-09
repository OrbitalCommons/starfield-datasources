//! Shared HTTP client utilities

use std::time::Duration;

use starfield::{Result, StarfieldError};

/// Build a blocking HTTP client with the given timeout.
///
/// All datasource crates use the same pattern for creating reqwest clients.
/// This centralizes that boilerplate.
pub fn build_http_client(timeout_secs: u64) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| StarfieldError::DataError(format!("Failed to create HTTP client: {}", e)))
}

/// Check that an HTTP response has a success status, returning a descriptive
/// error that includes `context` (typically the service or URL) on failure.
pub fn check_response_status(
    response: reqwest::blocking::Response,
    context: &str,
) -> Result<reqwest::blocking::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(StarfieldError::DataError(format!(
        "{} returned HTTP {}",
        context,
        response.status()
    )))
}

/// Assert that an HTTP endpoint is reachable via a HEAD request.
///
/// Any HTTP response (including 403, 404, 405) proves the server is up and
/// the hostname resolves. Only connection failures and timeouts cause a panic.
///
/// Intended for `#[ignore]` integration tests that verify upstream URLs.
pub fn assert_endpoint_reachable(url: &str) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client");

    client
        .head(url)
        .send()
        .unwrap_or_else(|e| panic!("HEAD request failed for {}: {}", url, e));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_http_client() {
        let client = build_http_client(30);
        assert!(client.is_ok());
    }
}
