//! Integration tests for DE440 ephemeris
//!
//! These tests download the DE440 BSP file (~114 MB) and verify that the
//! jplephem reader can load it, parse segments, and compute planetary
//! positions that match expected values.
//!
//! Run with: cargo test -p starfield-jplephem --test de440_integration -- --ignored

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use starfield_jplephem::SpiceKernel;

const DE440_URL: &str = "https://ssd.jpl.nasa.gov/ftp/eph/planets/bsp/de440.bsp";

/// Cache directory for downloaded test data
fn cache_dir() -> PathBuf {
    let dir = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".cache")
        .join("starfield");
    fs::create_dir_all(&dir).expect("Failed to create cache dir");
    dir
}

/// Download DE440 if not already cached, return path to the file.
///
/// Uses a per-thread temp file name to avoid races when tests run in parallel.
fn ensure_de440() -> PathBuf {
    let path = cache_dir().join("de440.bsp");
    if path.exists() && fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return path;
    }

    eprintln!("Downloading DE440 (~114 MB) ...");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("Failed to build HTTP client");

    let mut response = client
        .get(DE440_URL)
        .send()
        .expect("Failed to download DE440");
    assert!(
        response.status().is_success(),
        "DE440 download returned HTTP {}",
        response.status()
    );

    let temp_path = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file =
        std::io::BufWriter::new(fs::File::create(&temp_path).expect("Failed to create temp file"));

    let mut buffer = [0u8; 131_072];
    loop {
        let n = response.read(&mut buffer).expect("Failed to read response");
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])
            .expect("Failed to write to file");
    }
    file.flush().expect("Failed to flush file");
    drop(file);

    // Another thread may have finished first — that's fine
    if let Err(e) = fs::rename(&temp_path, &path) {
        if path.exists() {
            let _ = fs::remove_file(&temp_path);
        } else {
            panic!("Failed to rename temp file: {e}");
        }
    }
    eprintln!("Saved DE440 to {}", path.display());
    path
}

#[test]
#[ignore]
fn de440_loads_and_has_expected_segments() {
    let path = ensure_de440();
    let kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    let segments = &kernel.spk().segments;

    // DE440 contains: 9 planet barycenters (1-9), Sun (10), Moon (301),
    // Earth (399), and nutations/librations — at least 13 segments.
    // Unlike DE421, DE440 does NOT include individual planet bodies
    // (199, 299, 499) because for single-body systems (Mercury, Venus, Mars)
    // the barycenter IS the planet.
    assert!(
        segments.len() >= 13,
        "DE440 should have at least 13 segments, got {}",
        segments.len()
    );

    let expected_pairs: &[(i32, i32)] = &[
        (0, 1),   // SSB -> Mercury Barycenter
        (0, 2),   // SSB -> Venus Barycenter
        (0, 3),   // SSB -> Earth-Moon Barycenter
        (0, 4),   // SSB -> Mars Barycenter
        (0, 5),   // SSB -> Jupiter Barycenter
        (0, 6),   // SSB -> Saturn Barycenter
        (0, 7),   // SSB -> Uranus Barycenter
        (0, 8),   // SSB -> Neptune Barycenter
        (0, 9),   // SSB -> Pluto Barycenter
        (0, 10),  // SSB -> Sun
        (3, 301), // EMB -> Moon
        (3, 399), // EMB -> Earth
    ];

    for &(center, target) in expected_pairs {
        assert!(
            kernel.spk().get_segment(center, target).is_ok(),
            "DE440 missing segment center={center} target={target}"
        );
    }
}

#[test]
#[ignore]
fn de440_date_range_covers_1550_to_2650() {
    let path = ensure_de440();
    let kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    let segments = &kernel.spk().segments;
    let min_jd = segments.iter().map(|s| s.start_jd).fold(f64::MAX, f64::min);
    let max_jd = segments.iter().map(|s| s.end_jd).fold(f64::MIN, f64::max);

    // DE440 covers 1550-Jan-01 to 2650-Jan-22
    // JD 2287184.5 = ~1550-01-01
    // JD 2688976.5 = ~2650-01-22
    assert!(
        min_jd < 2_290_000.0,
        "DE440 start JD {min_jd} should be before ~2290000 (year 1550)"
    );
    assert!(
        max_jd > 2_688_000.0,
        "DE440 end JD {max_jd} should be after ~2688000 (year 2650)"
    );
}

#[test]
#[ignore]
fn de440_earth_at_j2000_roughly_1au() {
    let path = ensure_de440();
    let mut kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    let state = kernel
        .compute_at_jd("earth", 2451545.0)
        .expect("Failed to compute Earth position");

    let dist_au =
        (state.position.x.powi(2) + state.position.y.powi(2) + state.position.z.powi(2)).sqrt();

    assert!(
        dist_au > 0.98 && dist_au < 1.02,
        "Earth should be ~1 AU from SSB at J2000, got {dist_au}"
    );

    let speed_au_day = state.velocity.norm();
    assert!(
        speed_au_day > 0.015 && speed_au_day < 0.020,
        "Earth orbital speed should be ~0.017 AU/day, got {speed_au_day}"
    );
}

#[test]
#[ignore]
fn de440_all_bodies_nonzero_at_j2000() {
    let path = ensure_de440();
    let mut kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    // DE440 provides barycenters, not individual planet bodies (except Earth/Moon).
    // For single-body systems like Mercury, Venus, Mars, the barycenter equals the planet.
    let bodies = [
        "sun",
        "mercury barycenter",
        "venus barycenter",
        "earth",
        "moon",
        "mars barycenter",
        "jupiter barycenter",
        "saturn barycenter",
        "uranus barycenter",
        "neptune barycenter",
        "pluto barycenter",
    ];

    for name in bodies {
        let state = kernel
            .compute_at_jd(name, 2451545.0)
            .unwrap_or_else(|e| panic!("Failed to compute {name}: {e}"));

        let dist_au =
            (state.position.x.powi(2) + state.position.y.powi(2) + state.position.z.powi(2)).sqrt();

        assert!(
            dist_au > 0.0 && dist_au.is_finite(),
            "{name} distance from SSB should be positive and finite, got {dist_au}"
        );

        assert!(
            state.velocity.norm() > 0.0 && state.velocity.norm().is_finite(),
            "{name} velocity should be positive and finite"
        );
    }
}

#[test]
#[ignore]
fn de440_mars_barycenter_reasonable_distance_range() {
    let path = ensure_de440();
    let mut kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    // Mars barycenter orbits at ~1.38-1.67 AU from the Sun.
    // From SSB the distance is similar.
    let dates = [
        2451545.0, // 2000-01-01
        2451910.0, // 2001-01-01
        2452275.0, // 2002-01-01
        2459580.0, // 2022-01-01
    ];

    for jd in dates {
        let state = kernel
            .compute_at_jd("mars barycenter", jd)
            .unwrap_or_else(|e| panic!("Failed to compute Mars barycenter at JD {jd}: {e}"));

        let dist_au =
            (state.position.x.powi(2) + state.position.y.powi(2) + state.position.z.powi(2)).sqrt();

        assert!(
            dist_au > 1.3 && dist_au < 1.7,
            "Mars barycenter should be 1.3-1.7 AU from SSB at JD {jd}, got {dist_au}"
        );
    }
}

#[test]
#[ignore]
fn de440_comments_readable() {
    let path = ensure_de440();
    let kernel = SpiceKernel::open(&path).expect("Failed to open DE440");

    let comments = kernel
        .spk()
        .comments()
        .expect("Failed to read DE440 comments");

    assert!(
        comments.contains("DE440"),
        "DE440 comments should mention DE440, got: {}",
        &comments[..200.min(comments.len())]
    );
}
