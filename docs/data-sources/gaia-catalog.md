# Gaia Star Catalog

## 1. Overview

Gaia is a space observatory operated by the European Space Agency (ESA), launched on 19 December 2013. Its mission is to create the most precise three-dimensional map of the Milky Way by measuring positions, distances, and motions of stars with unprecedented accuracy.

### Data Releases

| Release | Date | Sources | Notes |
|---------|------|---------|-------|
| DR1 | September 2016 | ~1.1 billion | Positions and G-band magnitudes; limited parallaxes and proper motions (TGAS subset of ~2 million) |
| DR2 | April 2018 | ~1.7 billion | Five-parameter astrometry, radial velocities for ~7 million stars |
| EDR3 | December 2020 | ~1.8 billion | Improved astrometry and photometry; same source list as full DR3 |
| DR3 | June 2022 | ~1.8 billion | Full release: astrophysical parameters, variable star classifications, spectra, non-stellar objects |
| DR4 | Expected ~2026 | TBD | Full mission data, improved time-series, exoplanet orbits |

### Precision

Gaia achieves astrometric precision that varies with source brightness:

| G magnitude | Position precision | Parallax precision | Proper motion precision |
|-------------|-------------------|-------------------|------------------------|
| < 15 | ~20 microarcseconds | ~20 microarcseconds | ~20 microarcseconds/yr |
| 17 | ~100 microarcseconds | ~100 microarcseconds | ~70 microarcseconds/yr |
| 20 | ~700 microarcseconds | ~700 microarcseconds | ~500 microarcseconds/yr |
| 21 | ~2000 microarcseconds | ~2000 microarcseconds | ~1500 microarcseconds/yr |

### Reference Epoch

- **DR1**: J2015.0
- **DR2 and later**: J2016.0

All positions, parallaxes, and proper motions in DR2+ are given at epoch J2016.0 (JD 2457389.0 TDB, which corresponds to 2016-01-01T12:00:00 TDB). To obtain positions at a different epoch, proper motion must be applied.

### Coordinate System

Gaia uses the International Celestial Reference System (ICRS), materialized by the International Celestial Reference Frame (ICRF). Positions are reported as right ascension (ra) and declination (dec) in this frame.

---

## 2. Data Access

### Gaia Archive

The primary access point is the ESA Gaia Archive:

- **Main portal**: https://gea.esac.esa.int/archive/
- **TAP endpoint**: https://gea.esac.esa.int/tap-server/tap
- **Bulk download CDN**: https://cdn.gea.esac.esa.int/Gaia/

### Access Methods

#### TAP/ADQL Queries

The archive exposes a Table Access Protocol (TAP) service that accepts queries written in Astronomical Data Query Language (ADQL). This is the most flexible method for obtaining specific subsets of the catalog.

Example using `curl`:

```bash
curl -X POST "https://gea.esac.esa.int/tap-server/tap/sync" \
  --data "REQUEST=doQuery" \
  --data "LANG=ADQL" \
  --data "FORMAT=csv" \
  --data "QUERY=SELECT TOP 100 source_id, ra, dec, parallax, phot_g_mean_mag FROM gaiadr3.gaia_source WHERE phot_g_mean_mag < 6.0" \
  -o bright_stars.csv
```

#### Bulk Download via HTTP

Pre-partitioned CSV files are available for direct download from the CDN. Files are organized by data release and table:

```
https://cdn.gea.esac.esa.int/Gaia/gdr3/gaia_source/
https://cdn.gea.esac.esa.int/Gaia/gdr2/gaia_source/csv/
https://cdn.gea.esac.esa.int/Gaia/gdr1/gaia_source/csv/
```

Each release directory contains gzipped CSV files partitioned by HEALPix index or source ID range, along with an `MD5SUM.txt` file for integrity verification.

#### File Naming Conventions

- **DR1**: `GaiaSource_000-000-000.csv.gz` through `GaiaSource_000-020-XXX.csv.gz`
- **DR3**: `GaiaSource_XXXX.csv.gz` where `XXXX` is a zero-padded partition number

### How Starfield Accesses Gaia Data

The starfield library currently targets **Gaia DR1** via the bulk download CDN. The downloader implementation lives in `src/data/gaia_downloader.rs` and works as follows:

1. **Index discovery**: Fetches the HTML directory listing at the DR1 CSV base URL and parses out filenames matching `GaiaSource_\d{3}-\d{3}-\d{3}\.csv\.gz` using a regex.

2. **Download with caching**: Each gzipped CSV file is downloaded to a local cache directory at `~/.cache/starfield/gaia/`. Files are first written to a `.tmp` path and renamed atomically on completion to prevent partial downloads from corrupting the cache.

3. **MD5 verification**: An `MD5SUM.txt` file is downloaded from the archive and used to verify the integrity of each downloaded file.

4. **Streaming decompression**: The gzipped files are kept in compressed form on disk. The `GaiaCatalog::from_file()` method detects `.gz` extensions and applies `flate2::read::GzDecoder` for transparent streaming decompression during parsing.

5. **Catalog merging**: Multiple downloaded files can be merged into a single `GaiaCatalog` using the `merge()` method. When merging, duplicate `source_id` entries are resolved by keeping the entry from the first catalog (the receiver).

### Cache Layout

```
~/.cache/starfield/gaia/
    MD5SUM.txt
    GaiaSource_000-000-000.csv.gz
    GaiaSource_000-000-001.csv.gz
    ...
```

---

## 3. Data Fields

### Fields Parsed by Starfield

The current `GaiaEntry` struct extracts the following fields from CSV files. All fields listed as "required" cause the row to be skipped if parsing fails.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `source_id` | `u64` | Yes | Unique source identifier within the Gaia catalog |
| `solution_id` | `u64` | Yes | Identifier of the processing pipeline solution |
| `ra` | `f64` | Yes | Right ascension in degrees (ICRS, at reference epoch) |
| `dec` | `f64` | Yes | Declination in degrees (ICRS, at reference epoch) |
| `ra_error` | `f64` | Yes | Standard error in RA (milliarcseconds) |
| `dec_error` | `f64` | Yes | Standard error in Dec (milliarcseconds) |
| `parallax` | `Option<f64>` | No | Absolute stellar parallax (milliarcseconds). Empty for sources without astrometric solution. |
| `parallax_error` | `Option<f64>` | No | Standard error of parallax (milliarcseconds) |
| `pmra` | `Option<f64>` | No | Proper motion in RA direction, `mu_alpha * cos(delta)` (milliarcseconds/year) |
| `pmdec` | `Option<f64>` | No | Proper motion in Dec direction (milliarcseconds/year) |
| `phot_g_mean_mag` | `f64` | Yes | G-band mean magnitude (Vega system). Used for magnitude filtering. |
| `phot_g_mean_flux` | `f64` | Yes | G-band mean flux (electrons/second) |
| `phot_variable_flag` | `String` | Yes | Photometric variability flag: `"NOT_AVAILABLE"`, `"CONSTANT"`, or `"VARIABLE"` |
| `l` | `f64` | Yes | Galactic longitude (degrees) |
| `b` | `f64` | Yes | Galactic latitude (degrees) |
| `ecl_lon` | `f64` | Yes | Ecliptic longitude (degrees) |
| `ecl_lat` | `f64` | Yes | Ecliptic latitude (degrees) |

### Additional DR3 Fields (Not Yet Parsed)

These fields are available in Gaia DR3 and may be useful for future starfield features:

#### Photometry

| Field | Type | Description |
|-------|------|-------------|
| `phot_g_mean_flux_over_error` | `f64` | G-band flux divided by its error (signal-to-noise ratio) |
| `phot_bp_mean_mag` | `f64` | Integrated BP-band mean magnitude |
| `phot_bp_mean_flux` | `f64` | Integrated BP-band mean flux (electrons/second) |
| `phot_rp_mean_mag` | `f64` | Integrated RP-band mean magnitude |
| `phot_rp_mean_flux` | `f64` | Integrated RP-band mean flux (electrons/second) |
| `bp_rp` | `f64` | BP - RP color index (mag) |
| `bp_g` | `f64` | BP - G color index (mag) |
| `g_rp` | `f64` | G - RP color index (mag) |

The BP/RP color indices are essential for converting Gaia photometry to other photometric systems (e.g., Johnson-Cousins V, B-V). The current `approx_v_magnitude()` method on `GaiaEntry` returns `phot_g_mean_mag` directly, noting that precise V-band conversion requires color information.

#### Astrometric Quality

| Field | Type | Description |
|-------|------|-------------|
| `astrometric_excess_noise` | `f64` | Excess noise in the astrometric solution (mas). Non-zero values indicate the source may not be well-modeled as a single point source. |
| `astrometric_excess_noise_sig` | `f64` | Significance of excess noise (dimensionless). Values > 2 suggest the excess noise is statistically significant. |
| `ruwe` | `f64` | Renormalized Unit Weight Error. Values near 1.0 indicate a well-behaved single-star solution. Values > 1.4 suggest binarity, extended sources, or problematic solutions. |
| `astrometric_params_solved` | `i32` | Number of astrometric parameters solved: 2 (position only), 5 (full 5-parameter), 6 (pseudo-color added) |
| `ipd_gof_harmonic_amplitude` | `f64` | Image Parameter Determination goodness-of-fit harmonic amplitude. High values indicate extended or resolved sources. |
| `astrometric_chi2_al` | `f64` | Chi-squared value of the astrometric solution |
| `astrometric_n_good_obs_al` | `i32` | Number of good along-scan observations used |

#### Radial Velocity

| Field | Type | Description |
|-------|------|-------------|
| `radial_velocity` | `f64` | Spectroscopic barycentric radial velocity (km/s). Available for ~33 million sources in DR3 (mostly G_RVS < 14). |
| `radial_velocity_error` | `f64` | Standard error of radial velocity (km/s) |
| `rv_template_teff` | `f64` | Effective temperature of the template used for RV cross-correlation (K) |
| `rv_nb_transits` | `i32` | Number of transits used for radial velocity determination |

#### Astrophysical Parameters (GSP-Phot)

| Field | Type | Description |
|-------|------|-------------|
| `teff_gspphot` | `f64` | Effective temperature from GSP-Phot (K). Available for ~470 million sources in DR3. |
| `logg_gspphot` | `f64` | Surface gravity from GSP-Phot (log(cm/s^2)). Ranges from ~0 (supergiants) to ~5 (main sequence). |
| `mh_gspphot` | `f64` | Metallicity [M/H] from GSP-Phot (dex). Solar metallicity = 0.0. |
| `distance_gspphot` | `f64` | Distance estimate from GSP-Phot (parsecs). Uses both parallax and photometry, so can differ from simple 1/parallax inversion. |
| `azero_gspphot` | `f64` | Monochromatic extinction A_0 at 541.4 nm from GSP-Phot (mag) |
| `ag_gspphot` | `f64` | Extinction in G band from GSP-Phot (mag) |
| `ebpminrp_gspphot` | `f64` | Reddening E(BP-RP) from GSP-Phot (mag) |

#### Source Classification

| Field | Type | Description |
|-------|------|-------------|
| `classprob_dsc_combmod_quasar` | `f64` | Probability of being a quasar (0-1) |
| `classprob_dsc_combmod_galaxy` | `f64` | Probability of being a galaxy (0-1) |
| `classprob_dsc_combmod_star` | `f64` | Probability of being a star (0-1) |
| `in_qso_candidates` | `bool` | Whether the source appears in the QSO candidates table |
| `in_galaxy_candidates` | `bool` | Whether the source appears in the galaxy candidates table |

#### Proper Motion Derived Quantities

| Field | Type | Description |
|-------|------|-------------|
| `pm` | `f64` | Total proper motion (mas/yr), computed as `sqrt(pmra^2 + pmdec^2)` |
| `pmra_error` | `f64` | Standard error of proper motion in RA (mas/yr) |
| `pmdec_error` | `f64` | Standard error of proper motion in Dec (mas/yr) |
| `pmra_pmdec_corr` | `f64` | Correlation coefficient between pmra and pmdec |
| `ra_parallax_corr` | `f64` | Correlation coefficient between RA and parallax |
| `dec_parallax_corr` | `f64` | Correlation coefficient between Dec and parallax |

---

## 4. How Starfield Uses It

### Parsing Pipeline

The `GaiaCatalog` struct in `src/catalogs/gaia.rs` implements the following parsing approach:

1. **Header-driven column mapping**: The first line of the CSV is parsed to build a column-name-to-index map. This makes the parser resilient to column reordering across different Gaia data files. Column lookup is done via a `find_column()` closure that returns a `Result` with a descriptive error if a required column is missing.

2. **Line-by-line parsing**: Each subsequent line is split on commas and fields are extracted by their column index. Lines with insufficient columns are silently skipped.

3. **Magnitude filtering at parse time**: The G-band magnitude (`phot_g_mean_mag`) is checked against the caller-supplied `mag_limit` immediately after parsing. Stars fainter than the limit are discarded before allocating a `GaiaEntry`, reducing memory usage for large files.

4. **Optional field handling**: Fields like `parallax`, `parallax_error`, `pmra`, and `pmdec` are parsed as `Option<f64>`. Empty CSV fields produce `None` values rather than causing the row to be skipped.

5. **Storage**: Parsed entries are stored in a `HashMap<u64, GaiaEntry>` keyed by `source_id` for O(1) lookup.

### Loading Code Path

```rust
// Load a single file with magnitude limit
let catalog = GaiaCatalog::from_file("path/to/GaiaSource_000-000-000.csv.gz", 12.0)?;

// Load and merge multiple files
let mut catalog = GaiaCatalog::new();
for path in downloaded_paths {
    let chunk = GaiaCatalog::from_file(&path, 12.0)?;
    catalog.merge(chunk)?;
}
```

### Download Chunking Strategy

The downloader in `src/data/gaia_downloader.rs` processes files sequentially:

1. `list_gaia_files()` fetches the directory index and extracts all filenames.
2. `download_gaia_catalog(max_files)` iterates through the file list, downloading and verifying each one. The `max_files` parameter allows limiting downloads for testing or partial catalog use.
3. Each file is downloaded with a 10-minute HTTP timeout and an 8 KB buffer, with progress reported every 5 MB.
4. Failed downloads are logged but do not halt the process; remaining files continue downloading.

### StarCatalog Trait Implementation

`GaiaCatalog` implements the `StarCatalog` trait, providing:

- `get_star(id)`: Lookup by `source_id` (cast to `u64`)
- `stars()`: Iterator over all `GaiaEntry` values
- `star_data()`: Converts entries to the generic `StarData` format (id, ra, dec, magnitude). Note: `b_v` color index is set to `None` since Gaia does not directly provide Johnson B-V.
- `brighter_than(magnitude)`: Filter via `StarData` magnitude
- `stars_in_field(ra, dec, fov)`: Cone search using angular distance

### Derived Calculations

`GaiaEntry` provides two derived computation methods:

- **`unit_vector()`**: Converts RA/Dec to a unit vector in ICRS Cartesian coordinates using the standard spherical-to-Cartesian transformation.
- **`cartesian_position()`**: Returns a 3D position in parsecs by scaling the unit vector by `1000.0 / parallax_mas`. Returns `None` if parallax is absent or non-positive.

---

## 5. Query Strategies

### ADQL Syntax

ADQL (Astronomical Data Query Language) is a SQL-like language for querying astronomical databases. It extends SQL with geometric functions for celestial coordinates.

#### Cone Search (Circle on the Sky)

Find all sources within a radius of a given position:

```sql
SELECT source_id, ra, dec, parallax, phot_g_mean_mag
FROM gaiadr3.gaia_source
WHERE 1 = CONTAINS(
    POINT('ICRS', ra, dec),
    CIRCLE('ICRS', 83.633, -5.55, 1.0)
)
```

This queries a 1-degree radius circle centered on the Orion Nebula region.

#### Box Search (Rectangle on the Sky)

```sql
SELECT source_id, ra, dec, phot_g_mean_mag
FROM gaiadr3.gaia_source
WHERE ra BETWEEN 80.0 AND 85.0
  AND dec BETWEEN -10.0 AND 0.0
```

For regions not crossing the RA=0/360 boundary this is simpler than a polygon query. For regions crossing RA=0, use `OR` logic or the `POLYGON` function.

#### Magnitude-Limited Query

```sql
SELECT source_id, ra, dec, parallax, pmra, pmdec,
       phot_g_mean_mag, phot_bp_mean_mag, phot_rp_mean_mag,
       radial_velocity, teff_gspphot
FROM gaiadr3.gaia_source
WHERE phot_g_mean_mag < 10.0
ORDER BY phot_g_mean_mag ASC
```

#### Nearby Stars (Parallax-Based Distance Filter)

```sql
SELECT source_id, ra, dec, parallax, pmra, pmdec, phot_g_mean_mag,
       1000.0/parallax AS distance_pc
FROM gaiadr3.gaia_source
WHERE parallax > 0
  AND parallax_over_error > 10
  AND parallax > 50.0  -- closer than 20 parsecs
ORDER BY parallax DESC
```

The `parallax_over_error > 10` filter ensures reliable distance estimates.

#### Cross-Match with Other Catalogs

Gaia DR3 provides pre-computed cross-matches with other major catalogs:

```sql
-- Cross-match with Hipparcos
SELECT g.source_id, g.ra, g.dec, g.phot_g_mean_mag,
       h.hip, h.hpmag
FROM gaiadr3.gaia_source AS g
JOIN gaiadr3.hipparcos2_best_neighbour AS h
  ON g.source_id = h.source_id
WHERE g.phot_g_mean_mag < 6.0

-- Cross-match with Tycho-2
SELECT g.source_id, t.original_ext_source_id AS tycho_id
FROM gaiadr3.gaia_source AS g
JOIN gaiadr3.tycho2tdsc_merge_best_neighbour AS t
  ON g.source_id = t.source_id
```

Available cross-match tables include:
- `hipparcos2_best_neighbour`
- `tycho2tdsc_merge_best_neighbour`
- `panstarrs1_best_neighbour`
- `sdssdr13_best_neighbour`
- `allwise_best_neighbour`
- `tmass_psc_best_neighbour` (2MASS)

#### High Proper Motion Stars

```sql
SELECT source_id, ra, dec, pmra, pmdec,
       SQRT(pmra*pmra + pmdec*pmdec) AS total_pm,
       phot_g_mean_mag
FROM gaiadr3.gaia_source
WHERE pmra IS NOT NULL
  AND pmdec IS NOT NULL
  AND SQRT(pmra*pmra + pmdec*pmdec) > 500.0
ORDER BY total_pm DESC
```

### Pagination for Large Results

The Gaia TAP service limits synchronous queries to relatively small result sets (~3 million rows). For larger queries, use asynchronous mode:

1. Submit an asynchronous job via the TAP `/async` endpoint.
2. Poll the job status until completion.
3. Download the result file.

Alternatively, partition queries by sky region (HEALPix level) or source ID range:

```sql
-- Partition by HEALPix level 5 pixel
SELECT source_id, ra, dec, phot_g_mean_mag
FROM gaiadr3.gaia_source
WHERE source_id BETWEEN 0 AND 562949953421311  -- First HEALPix L12 range
  AND phot_g_mean_mag < 15.0
```

The `source_id` in Gaia encodes the HEALPix level-12 pixel index in its most significant bits, so range queries on `source_id` correspond to spatial regions on the sky.

---

## 6. Data Volume Considerations

### Full Catalog Sizes

| Release | Table | Compressed Size | Uncompressed Size | Sources |
|---------|-------|----------------|-------------------|---------|
| DR1 | `gaia_source` | ~12 GB (gzipped CSV) | ~55 GB | ~1.1 billion |
| DR3 | `gaia_source` | ~650 GB (gzipped CSV) | ~2.5 TB | ~1.8 billion |

### Practical Subsets by Magnitude Limit

Approximate source counts and download sizes for magnitude-limited subsets of DR3:

| G magnitude limit | Approx. sources | Approx. CSV size (uncompressed) | Approx. CSV size (gzipped) |
|-------------------|-----------------|-------------------------------|--------------------------|
| < 6.0 | ~9,000 | ~5 MB | ~1 MB |
| < 8.0 | ~80,000 | ~40 MB | ~8 MB |
| < 10.0 | ~650,000 | ~350 MB | ~70 MB |
| < 12.0 | ~5 million | ~2.5 GB | ~500 MB |
| < 14.0 | ~40 million | ~20 GB | ~4 GB |
| < 16.0 | ~250 million | ~120 GB | ~25 GB |
| < 18.0 | ~800 million | ~400 GB | ~80 GB |
| < 21.0 (full) | ~1.8 billion | ~2.5 TB | ~650 GB |

These numbers are approximate. For starfield's use cases (star field rendering, astrometry), magnitude limits of 12-16 are typically sufficient and keep data volumes manageable.

### Strategies for Reducing Data Volume

1. **Column selection**: Query only the columns you need. The full `gaia_source` table has 152 columns; starfield uses 17. Requesting fewer columns via ADQL dramatically reduces download size.

2. **Spatial partitioning**: Download only the sky regions relevant to your use case rather than the full sky.

3. **Quality filters**: Exclude low-quality measurements:
   ```sql
   WHERE parallax_over_error > 5
     AND ruwe < 1.4
     AND astrometric_excess_noise < 1.0
   ```

4. **Source type filtering**: Exclude non-stellar sources if only stars are needed:
   ```sql
   WHERE classprob_dsc_combmod_star > 0.9
   ```

---

## 7. Practical Notes

### Rate Limits and Etiquette

The Gaia Archive does not publish strict rate limits, but users should follow these guidelines:

- **Synchronous queries**: Limited to results under ~3 million rows (configurable by the archive). Queries exceeding this limit will fail and should be resubmitted as asynchronous jobs.
- **Concurrent connections**: Keep concurrent downloads to a reasonable number (2-4 simultaneous connections). The CDN can handle more, but aggressive parallelism may result in throttling or IP bans.
- **Query complexity**: Avoid extremely complex JOIN operations on the full `gaia_source` table. Use indexed columns (`source_id`, `ra`, `dec`, `phot_g_mean_mag`, `parallax`) in WHERE clauses.
- **Large result sets**: Use asynchronous TAP jobs for queries expected to return more than a few hundred thousand rows.

### Download Strategies for Large Datasets

1. **Incremental download**: The starfield downloader processes files sequentially, which is resilient to interruptions. Already-downloaded files are detected by the cache and skipped on retry. Use the `max_files` parameter to limit batch sizes.

2. **Checksum verification**: Always verify downloads against the `MD5SUM.txt` file provided in each data release directory. The starfield downloader does this automatically.

3. **Temporary file pattern**: Write downloads to a `.tmp` file first, then rename atomically. This prevents partially downloaded files from being treated as complete. The starfield downloader implements this pattern.

4. **Compression**: Keep gzipped files on disk and decompress during parsing. This roughly halves storage requirements with minimal performance cost due to the I/O-bound nature of CSV parsing.

### Cache Management

Starfield stores cached Gaia files at `~/.cache/starfield/gaia/`. To manage this cache:

- **List cached files**: Use `list_cached_gaia_files()` from `src/data/gaia_downloader.rs` to enumerate all cached `.csv` and `.csv.gz` files.
- **Clear cache**: Delete the `~/.cache/starfield/gaia/` directory to force re-download.
- **Disk usage**: Monitor cache size, especially if downloading large portions of the catalog. A full DR1 download is ~12 GB compressed.

### Known Limitations in Current Implementation

1. **DR1 only**: The downloader currently targets Gaia DR1 (`gdr1`). DR3 data must be obtained through manual download or ADQL queries and loaded via `GaiaCatalog::from_file()`.

2. **No ADQL client**: Starfield does not include a TAP/ADQL client. For custom queries, use external tools (e.g., `curl`, TOPCAT, `astroquery` in Python) to obtain CSV files, then load them with `GaiaCatalog::from_file()`.

3. **No B-V color**: The `StarData` conversion sets `b_v` to `None` because Gaia's native photometric bands (G, BP, RP) do not directly map to the Johnson B-V system. Conversion requires BP-RP color, which is not currently parsed.

4. **Sequential downloads**: Files are downloaded one at a time. Parallel downloads would improve throughput for full-catalog retrieval.

5. **No incremental merge during download**: The downloader returns a list of file paths. Building a catalog from many files requires loading and merging each one separately after download.

### Useful External Tools

- **TOPCAT** (http://www.star.bris.ac.uk/~mbt/topcat/): Desktop application for interactive catalog exploration and TAP queries.
- **astroquery** (Python): `astroquery.gaia` module provides programmatic TAP access.
- **Aladin Lite** (https://aladin.u-strasbg.fr/AladinLite/): Browser-based sky viewer with Gaia overlay support.
- **VizieR** (https://vizier.u-strasbg.fr/): Alternative access point for Gaia tables with pre-built queries.
