//! Shared utilities for starfield datasource crates
//!
//! Common helpers for HTTP clients, file caching, and downloads
//! used across multiple datasource implementations.

pub mod cache;
pub mod download;
pub mod http;

pub use cache::{cache_dir, ensure_cache_dir, ensure_cache_subdir, file_exists_and_not_empty};
pub use download::download_to_file;
pub use http::{build_http_client, check_response_status};
