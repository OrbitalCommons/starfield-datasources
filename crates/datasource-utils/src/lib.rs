//! Shared utilities for starfield datasource crates
//!
//! Common helpers for HTTP clients, file caching, and downloads
//! used across multiple datasource implementations.

pub mod http;

pub use http::{build_http_client, check_response_status};
