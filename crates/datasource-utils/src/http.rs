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

/// Assert that an HTTP endpoint responds to a HEAD request.
///
/// Accepts 2xx or 405 (Method Not Allowed — means the server is up but
/// rejects HEAD). Panics with a descriptive message on connection failure
/// or unexpected status codes.
///
/// Intended for `#[ignore]` integration tests that verify upstream URLs.
pub fn assert_endpoint_reachable(url: &str) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client");

    let resp = client
        .head(url)
        .send()
        .unwrap_or_else(|e| panic!("HEAD request failed for {}: {}", url, e));

    assert!(
        resp.status().is_success() || resp.status().as_u16() == 405,
        "Endpoint {} returned HTTP {}",
        url,
        resp.status()
    );
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
