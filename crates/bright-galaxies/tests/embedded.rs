//! Tests against the embedded bright-galaxy supplement + the from-file
//! parser path.

use std::io::Write;

use starfield_bright_galaxies::BrightGalaxyCatalog;
use starfield_gaia::Cone;

#[test]
fn embedded_loads_and_parses() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    assert!(
        cat.len() >= 30,
        "supplement should hold at least 30 entries, got {}",
        cat.len()
    );

    // M31 must be present and at the right position.
    let m31 = cat.get("M31").expect("M31 in supplement");
    assert!((m31.ra_deg - 10.6847).abs() < 0.01, "M31 RA off");
    assert!((m31.dec_deg - 41.2691).abs() < 0.01, "M31 Dec off");
    assert!(m31.mag_v < 4.0, "M31 should be naked-eye bright");
    assert!(
        m31.radius_sersic_arcsec > 100.0,
        "M31 R_e should be substantial"
    );
}

#[test]
fn every_entry_has_plausible_values() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    for g in cat.iter() {
        assert!(
            (-90.0..=90.0).contains(&g.dec_deg),
            "{}: dec out of range: {}",
            g.name,
            g.dec_deg
        );
        assert!(
            (0.0..360.0).contains(&g.ra_deg),
            "{}: ra out of range: {}",
            g.name,
            g.ra_deg
        );
        assert!(
            (-2.0..15.0).contains(&g.mag_v),
            "{}: mag_v out of plausible range: {}",
            g.name,
            g.mag_v
        );
        assert!(
            g.radius_sersic_arcsec > 0.0,
            "{}: radius must be positive",
            g.name
        );
        assert!(
            (0.3..=8.0).contains(&g.n_sersic),
            "{}: n_sersic out of plausible range: {}",
            g.name,
            g.n_sersic
        );
        assert!(
            (0.0..=1.0).contains(&g.ellipticity_sersic),
            "{}: ellipticity out of [0,1]: {}",
            g.name,
            g.ellipticity_sersic
        );
        assert!(
            (0.0..=180.0).contains(&g.pa_sersic_deg),
            "{}: PA out of [0,180]: {}",
            g.name,
            g.pa_sersic_deg
        );
        assert!(!g.morph_type.is_empty(), "{}: empty morph type", g.name);
    }
}

#[test]
fn local_group_landmarks_present() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    for name in ["M31", "M32", "M110", "M33", "LMC", "SMC"] {
        assert!(
            cat.get(name).is_some(),
            "missing Local Group landmark: {}",
            name
        );
    }
}

#[test]
fn virgo_cluster_headliners_present() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    for name in ["M87", "M86", "M84", "M49", "M60"] {
        assert!(cat.get(name).is_some(), "missing Virgo headliner: {}", name);
    }
}

#[test]
fn cone_filter_returns_only_inside_galaxies() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    // Cone around the Virgo cluster centre — should pull M84/M86/M87/M89 et al.
    let cone = Cone::from_degrees(187.7, 12.4, 3.0);
    let inside = cat.in_cone(&cone);
    let names: std::collections::HashSet<&str> = inside.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains("M87"), "M87 should be in Virgo cone");
    assert!(names.contains("M84"), "M84 should be in Virgo cone");
    assert!(names.contains("M86"), "M86 should be in Virgo cone");
    // And things far away should NOT be in there.
    assert!(!names.contains("M31"));
    assert!(!names.contains("LMC"));
}

#[test]
fn from_csv_file_round_trips() {
    let cat = BrightGalaxyCatalog::load_embedded().unwrap();
    // Write a custom file with one entry then re-parse.
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        f,
        "name,ra_deg,dec_deg,morph_type,mag_v,radius_sersic_arcsec,n_sersic,ellipticity_sersic,pa_sersic_deg,notes"
    )
    .unwrap();
    writeln!(
        f,
        "TestGalaxy,123.45,-67.89,Sb,9.5,42.0,2.0,0.30,90.0,unit test row"
    )
    .unwrap();
    f.flush().unwrap();
    let custom = BrightGalaxyCatalog::from_csv_file(f.path()).unwrap();
    assert_eq!(custom.len(), 1);
    let g = custom.get("TestGalaxy").unwrap();
    assert_eq!(g.ra_deg, 123.45);
    assert_eq!(g.dec_deg, -67.89);
    assert_eq!(g.notes, "unit test row");

    // The embedded one is still the bigger catalog.
    assert!(cat.len() > custom.len());
}

#[test]
fn rejects_bad_header() {
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(f, "name,ra,dec").unwrap(); // wrong column names
    writeln!(f, "Foo,0.0,0.0").unwrap();
    f.flush().unwrap();
    let err = BrightGalaxyCatalog::from_csv_file(f.path()).expect_err("should reject bad header");
    let msg = err.to_string();
    assert!(
        msg.contains("header columns must be"),
        "error should explain header schema, got: {}",
        msg
    );
}

#[test]
fn rejects_duplicate_names() {
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        f,
        "name,ra_deg,dec_deg,morph_type,mag_v,radius_sersic_arcsec,n_sersic,ellipticity_sersic,pa_sersic_deg,notes"
    )
    .unwrap();
    writeln!(f, "Twin,0.0,0.0,E0,10.0,30.0,4.0,0.1,0.0,first").unwrap();
    writeln!(f, "Twin,1.0,1.0,Sa,11.0,40.0,2.0,0.3,90.0,second").unwrap();
    f.flush().unwrap();
    let err = BrightGalaxyCatalog::from_csv_file(f.path()).expect_err("duplicate name");
    let msg = err.to_string();
    assert!(
        msg.contains("duplicate"),
        "error should mention duplicate, got: {}",
        msg
    );
}

#[test]
fn comment_and_blank_lines_skipped() {
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        f,
        "name,ra_deg,dec_deg,morph_type,mag_v,radius_sersic_arcsec,n_sersic,ellipticity_sersic,pa_sersic_deg,notes"
    )
    .unwrap();
    writeln!(f, "# this is a comment line").unwrap();
    writeln!(f).unwrap();
    writeln!(f, "Galaxy1,0.0,0.0,E0,10.0,30.0,4.0,0.1,0.0,").unwrap();
    writeln!(f, "  ").unwrap(); // whitespace-only
    writeln!(f, "Galaxy2,1.0,1.0,Sa,11.0,40.0,2.0,0.3,90.0,").unwrap();
    f.flush().unwrap();
    let cat = BrightGalaxyCatalog::from_csv_file(f.path()).unwrap();
    assert_eq!(cat.len(), 2);
}
