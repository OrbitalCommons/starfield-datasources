//! Integration tests that verify all broker URL constants resolve.
//!
//! These tests are marked `#[ignore]` because they require network access.
//! Run with: `cargo test -p starfield-rubin --test url_verification -- --ignored`

use starfield_datasource_utils::assert_endpoint_reachable;
use starfield_rubin::*;

#[test]
#[ignore]
fn test_all_broker_api_urls_reachable() {
    for (name, url) in all_broker_api_urls() {
        println!("Checking API: {} -> {}", name, url);
        assert_endpoint_reachable(url);
    }
}

#[test]
#[ignore]
fn test_all_broker_doc_urls_reachable() {
    for (name, url) in all_broker_doc_urls() {
        println!("Checking docs: {} -> {}", name, url);
        assert_endpoint_reachable(url);
    }
}

#[test]
fn test_doc_urls_match_code_constants() {
    let api_urls = all_broker_api_urls();
    let doc_urls = all_broker_doc_urls();

    // Verify we have the expected number of entries
    assert_eq!(
        api_urls.len(),
        7,
        "Expected 7 API URL entries (6 brokers + AMPEL split)"
    );
    assert_eq!(doc_urls.len(), 10, "Expected 10 doc URL entries");

    // Verify API URLs match their module constants
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "ALeRCE" && *u == alerce::API_BASE_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "ANTARES" && *u == antares::API_BASE_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "Fink" && *u == fink::API_BASE_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "Lasair" && *u == lasair::API_BASE_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "AMPEL catalog" && *u == ampel::CATALOG_API_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "AMPEL archive" && *u == ampel::ARCHIVE_API_URL));
    assert!(api_urls
        .iter()
        .any(|(n, u)| *n == "Babamul" && *u == babamul::API_BASE_URL));

    // Verify doc URLs match their module constants
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "ALeRCE docs" && *u == alerce::DOCS_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "ANTARES portal" && *u == antares::PORTAL_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "Fink portal" && *u == fink::PORTAL_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "Lasair portal" && *u == lasair::PORTAL_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "Pitt-Google docs" && *u == pitt_google::DOCS_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "AMPEL catalog docs" && *u == ampel::CATALOG_DOCS_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "AMPEL archive docs" && *u == ampel::ARCHIVE_DOCS_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "Babamul portal" && *u == babamul::PORTAL_URL));
    assert!(doc_urls
        .iter()
        .any(|(n, u)| *n == "Babamul docs" && *u == babamul::DOCS_URL));
}

#[test]
fn test_all_urls_are_https() {
    for (name, url) in all_broker_api_urls() {
        assert!(
            url.starts_with("https://"),
            "{} API URL should use HTTPS: {}",
            name,
            url
        );
    }
    for (name, url) in all_broker_doc_urls() {
        assert!(
            url.starts_with("https://"),
            "{} doc URL should use HTTPS: {}",
            name,
            url
        );
    }
}
