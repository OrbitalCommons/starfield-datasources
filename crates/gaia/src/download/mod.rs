//! HTTP downloading, MD5 verification, and local cache management.
//!
//! The [`Downloader`] is generic over the [`GaiaRelease`](crate::common::traits::GaiaRelease)
//! marker; per-release entry points are re-exported from the `dr1`/`dr2`/`dr3` modules.

pub mod client;

pub use client::Downloader;
