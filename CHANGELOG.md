# Changelog

## 0.2.0

### Removed

- Removed `starfield-jplephem` crate — jplephem has been reintegrated into the main `starfield` crate
- `starfield-horizons` now uses `starfield::jplephem::SpiceKernel` directly
- Removed `jplephem` feature flag from the `starfield-datasources` facade crate

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
