# Changelog

## 0.5.0 — Gaia DR1/DR2/DR3 multi-release support

### starfield-gaia 0.1.0 → 0.2.0 (breaking)

The crate now ships first-class loaders for **DR1, DR2, and DR3**. Each release has
its own `Entry` type exposing every field that release publishes, organized into
coherent sub-structs (BP/RP photometry, radial velocity, IPD quality, GSP-Phot,
datalink flags, etc.). The legacy top-level `GaiaCatalog` / `GaiaEntry` are gone.

- Per-release types: `Dr1Catalog` / `Dr1Entry`, `Dr2Catalog` / `Dr2Entry`, `Dr3Catalog` / `Dr3Entry`
- Shared `GaiaCore` (embedded in every entry) for astrometry + G-band photometry
- Generic `GaiaRelease` trait parameterizes the reader, downloader, and in-memory catalog
- Arrow CSV reader with typed per-release schema — missing/renamed columns become typed errors
- Automatic ECSV (`# %ECSV 1.0`) preamble stripping for DR3 files
- DR1 TGAS cross-ids supported via `load_tgas_block_map` + `Dr1Catalog::attach_tgas`
- Cargo features: `dr1`, `dr2`, `dr3` (default), `all-releases`

### Facade crate

- New `gaia-all` feature: forwards to `starfield-gaia/all-releases`
- Default `gaia` feature remains DR3-only

### Migration

| Old API (0.1.0) | New API (0.2.0) |
|---|---|
| `GaiaCatalog::from_file(path, mag)` | `Dr1Catalog::from_csv_file(path, mag)` (or `Dr2Catalog` / `Dr3Catalog`) |
| `GaiaEntry { ra, dec, phot_g_mean_mag, parallax, pmra, pmdec, ... }` | `entry.core.{ra, dec, phot_g_mean_mag, parallax, pmra, pmdec, ...}` |
| `GaiaEntry::unit_vector()`, `cartesian_position()` | `entry.core.unit_vector()`, `entry.core.cartesian_position()` |
| `download_gaia_file(name)` / `download_gaia_catalog(max)` | `dr3::download_file(name)` / `dr3::download_all(max)` (or `dr1::` / `dr2::`) |
| `list_cached_gaia_files()` | `dr3::list_cached()` (per release) |

## 0.4.0

### Removed

- Removed `starfield-jplephem` crate — jplephem has been reintegrated into the main `starfield` crate
- `starfield-horizons` now uses `starfield::jplephem::SpiceKernel` directly
- Removed `jplephem` feature flag from the `starfield-datasources` facade crate

## 0.3.0 — Rubin Observatory Alert Brokers

Added `starfield-rubin` crate with typed Rust clients for all seven Vera C. Rubin Observatory LSST community alert brokers.

### Brokers

| Broker | Auth | Module |
|--------|------|--------|
| ALeRCE | None | `alerce` |
| ANTARES | None (search) | `antares` |
| Fink | None | `fink` |
| Lasair | API token | `lasair` |
| Pitt-Google | GCP credentials | `pitt_google` |
| AMPEL | Bearer token (archive) / None (catalog) | `ampel` |
| Babamul | API token (invitation only) | `babamul` |

### CI

- Added GitHub Actions workflow with `cargo check`, `cargo test`, `cargo fmt --check`
- Added URL verification job that checks all broker API and documentation URLs resolve

### Other

- Facade crate bumped to 0.3.0 with new `rubin` feature flag (enabled by default)

## 0.1.0 — Initial Release

Extracted all astronomical data source clients from the [starfield](https://github.com/OrbitalCommons/starfield) monolith into independent crates.

### Crates

| Crate | Description | Tests |
|---|---|---|
| `starfield-horizons` | NASA JPL HORIZONS API client (vectors, observer, elements, approach, SPK generation) | 50 |
| `starfield-sbdb` | NASA JPL Small-Body Database API client (11 endpoints: lookup, CAD, fireball, sentry, scout, mission design, radar, identification, observability, NHATS) | 52 |
| `starfield-gaia` | ESA Gaia star catalog loader and downloader (DR1 CSV/gzip) | 2 |
| `starfield-hipparcos` | Hipparcos star catalog loader and downloader | 9 |
| `starfield-datasources` | Facade crate re-exporting all of the above behind feature flags | — |

### Migration from `starfield`

The data source code was extracted from these locations in `starfield`:

| Old import (starfield) | New import (this repo) |
|---|---|
| `starfield::horizons::HorizonsClient` | `starfield_horizons::HorizonsClient` |
| `starfield::horizons::EphemerisRequest` | `starfield_horizons::EphemerisRequest` |
| `starfield::horizons::parser::*` | `starfield_horizons::parser::*` |
| `starfield::sbdb::SbdbClient` | `starfield_sbdb::SbdbClient` |
| `starfield::sbdb::types::*` | `starfield_sbdb::types::*` |
| `starfield::sbdb::query::SbdbQueryParams` | `starfield_sbdb::query::SbdbQueryParams` |
| `starfield::catalogs::gaia::GaiaCatalog` | `starfield_gaia::GaiaCatalog` |
| `starfield::data::gaia_downloader::*` | `starfield_gaia::downloader::*` |
| `starfield::catalogs::hipparcos::HipparcosCatalog` | `starfield_hipparcos::HipparcosCatalog` |
| `starfield::data::download_hipparcos` | `starfield_hipparcos::download_hipparcos` |

### Dependency Direction

- `starfield-horizons` depends on `starfield` (for `StarfieldError`, `Result`, `SpiceKernel`)
- `starfield-sbdb` depends on `starfield` (for `StarfieldError`, `Result`)
- `starfield-gaia` depends on `starfield` (for `StarfieldError`, `Result`, `StarCatalog`, `StarData`)
- `starfield-hipparcos` depends on `starfield` (for `StarfieldError`, `Result`, `StarCatalog`, `StarData`)

### Using the Facade Crate

Add `starfield-datasources` to get everything, or pick individual crates:

```toml
# Everything
[dependencies]
starfield-datasources = "0.1"

# Or pick what you need
[dependencies]
starfield-horizons = "0.1"
```

Feature flags on the facade crate: `horizons`, `sbdb`, `gaia`, `hipparcos` (all enabled by default).
