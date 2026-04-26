//! End-to-end excerpt test: write a synthetic DR3 fixture with known rows,
//! shard via every built-in `ShardKey`, and verify (a) every kept row lands
//! in some shard, (b) shard membership respects the rule, (c) shard files
//! round-trip back through `Dr3Catalog::from_csv_file`.
#![cfg(feature = "dr3")]

use std::collections::HashMap;
use std::io::Write;

use starfield::catalogs::StarCatalog;
use starfield_gaia::excerpt::{
    excerpt_csv_file, excerpt_csv_file_into, HashIdShard, HealpixShard, IdRangeShard, ShardKey,
    ShardedCsvWriter,
};
use starfield_gaia::{Dr3, Dr3Catalog};

const HEADER: &str = "source_id,solution_id,ref_epoch,random_index,designation,ra,ra_error,dec,dec_error,ra_dec_corr,parallax,parallax_error,parallax_over_error,pm,pmra,pmra_error,pmdec,pmdec_error,l,b,ecl_lon,ecl_lat,phot_g_mean_mag,phot_g_mean_flux,phot_g_mean_flux_error,phot_g_n_obs,phot_variable_flag,astrometric_n_obs_al,astrometric_excess_noise,astrometric_excess_noise_sig,astrometric_primary_flag,duplicated_source,matched_observations,astrometric_n_good_obs_al,astrometric_n_bad_obs_al,astrometric_gof_al,astrometric_chi2_al,astrometric_params_solved,visibility_periods_used,astrometric_sigma5d_max,nu_eff_used_in_astrometry,pseudocolour,pseudocolour_error,ruwe,ipd_gof_harmonic_amplitude,ipd_gof_harmonic_phase,ipd_frac_multi_peak,ipd_frac_odd_win,phot_bp_mean_mag,phot_bp_mean_flux,phot_bp_mean_flux_error,phot_bp_n_obs,phot_rp_mean_mag,phot_rp_mean_flux,phot_rp_mean_flux_error,phot_rp_n_obs,bp_rp,bp_g,g_rp,phot_bp_rp_excess_factor,phot_proc_mode,radial_velocity,radial_velocity_error,rv_method_used,rv_nb_transits,rv_expected_sig_to_noise,rv_amplitude_robust,rv_template_teff,rv_template_logg,rv_template_fe_h,rv_atm_param_origin,teff_gspphot,teff_gspphot_lower,teff_gspphot_upper,logg_gspphot,mh_gspphot,distance_gspphot,distance_gspphot_lower,distance_gspphot_upper,azero_gspphot,ag_gspphot,ebpminrp_gspphot,libname_gspphot,has_xp_continuous,has_xp_sampled,has_rvs,has_epoch_photometry,has_epoch_rv,has_mcmc_gspphot,has_mcmc_msc,in_qso_candidates,in_galaxy_candidates,in_andromeda_survey,non_single_star,classprob_dsc_combmod_quasar,classprob_dsc_combmod_galaxy,classprob_dsc_combmod_star";

fn row_from(overrides: &HashMap<&str, &str>) -> String {
    HEADER
        .split(',')
        .map(|c| overrides.get(c).copied().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(",")
}

/// Build a fixture with N synthetic rows whose source_ids cover a wide range
/// and whose ra/dec are scattered across the sphere.
fn write_n_row_fixture(n: usize) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(f, "{}", HEADER).unwrap();
    for i in 0..n {
        let id = (i as u64).wrapping_mul(0x123_4567_89AB_CDEF);
        let mag = 4.0 + (i as f64) * 0.05;
        let ra = (i as f64 * 7.13) % 360.0;
        let dec = -85.0 + (i as f64 * 1.91) % 170.0;
        let row = row_from(&HashMap::from([
            ("source_id", id.to_string().as_str()),
            ("solution_id", "1635721458409799680"),
            ("ref_epoch", "2016.0"),
            ("ra", ra.to_string().as_str()),
            ("ra_error", "0.04"),
            ("dec", dec.to_string().as_str()),
            ("dec_error", "0.04"),
            ("l", "0.0"),
            ("b", "0.0"),
            ("ecl_lon", "0.0"),
            ("ecl_lat", "0.0"),
            ("phot_g_mean_mag", mag.to_string().as_str()),
            ("phot_variable_flag", "NOT_AVAILABLE"),
        ]));
        writeln!(f, "{}", row).unwrap();
    }
    f.flush().unwrap();
    f
}

#[test]
fn excerpt_round_trip_with_hash_shard() {
    let fixture = write_n_row_fixture(200);
    let out = tempfile::tempdir().unwrap();
    let summary = excerpt_csv_file::<Dr3, _, _>(
        fixture.path(),
        f64::INFINITY,
        out.path(),
        HashIdShard { num_shards: 8 },
        |_| true,
    )
    .expect("excerpt");

    assert_eq!(summary.input_rows, 200);
    assert_eq!(summary.kept_rows, 200);
    assert_eq!(summary.per_shard_counts.iter().sum::<u64>(), 200);

    // Reload every shard and verify every original source_id is present.
    let mut all_ids = Vec::new();
    for path in summary.written_paths() {
        let cat = Dr3Catalog::from_csv_file(path, f64::INFINITY).expect("reload shard");
        for s in cat.stars() {
            all_ids.push(s.core.source_id);
        }
    }
    all_ids.sort();
    let mut expected: Vec<u64> = (0..200u64)
        .map(|i| i.wrapping_mul(0x123_4567_89AB_CDEF))
        .collect();
    expected.sort();
    assert_eq!(all_ids, expected, "round-trip lost or duplicated rows");
}

#[test]
fn excerpt_predicate_filters_rows() {
    let fixture = write_n_row_fixture(100);
    let out = tempfile::tempdir().unwrap();
    let summary = excerpt_csv_file::<Dr3, _, _>(
        fixture.path(),
        f64::INFINITY,
        out.path(),
        HashIdShard { num_shards: 4 },
        |e| e.core.phot_g_mean_mag < 5.0, // mag < 5 → first 20 rows (4.0..4.95)
    )
    .expect("excerpt");
    assert_eq!(summary.kept_rows, 20);
}

#[test]
fn id_range_shard_buckets_monotonically() {
    let s = IdRangeShard { num_shards: 4 };
    // Construct fake entries by parsing them via from_csv_file with single rows.
    // Simpler: test the shard arithmetic directly.
    use starfield_gaia::common::core::GaiaCore;
    use starfield_gaia::dr3::Dr3Entry;
    let make = |id: u64| Dr3Entry {
        core: GaiaCore {
            source_id: id,
            solution_id: 0,
            ref_epoch: 2016.0,
            random_index: None,
            ra: 0.0,
            ra_error: 0.0,
            dec: 0.0,
            dec_error: 0.0,
            ra_dec_corr: None,
            parallax: None,
            parallax_error: None,
            pmra: None,
            pmra_error: None,
            pmdec: None,
            pmdec_error: None,
            l: 0.0,
            b: 0.0,
            ecl_lon: 0.0,
            ecl_lat: 0.0,
            phot_g_mean_mag: 0.0,
            phot_g_mean_flux: None,
            phot_g_mean_flux_error: None,
            phot_g_n_obs: None,
            phot_variable_flag: Default::default(),
            astrometric_n_obs_al: None,
            astrometric_excess_noise: None,
            astrometric_excess_noise_sig: None,
            astrometric_primary_flag: None,
            duplicated_source: None,
            matched_observations: None,
        },
        designation: None,
        pm: None,
        parallax_over_error: None,
        astrometric_extra: Default::default(),
        ipd: Default::default(),
        bp_rp: None,
        radial_velocity: None,
        gspphot: None,
        data_links: Default::default(),
        classifications: Default::default(),
    };

    assert_eq!(
        <IdRangeShard as ShardKey<Dr3Entry>>::shard_of(&s, &make(0)),
        0
    );
    assert_eq!(
        <IdRangeShard as ShardKey<Dr3Entry>>::shard_of(&s, &make(u64::MAX)),
        3
    );
    assert_eq!(
        <IdRangeShard as ShardKey<Dr3Entry>>::shard_of(&s, &make(u64::MAX / 2)),
        1
    );
    let prev = <IdRangeShard as ShardKey<Dr3Entry>>::shard_of(&s, &make(0));
    let next = <IdRangeShard as ShardKey<Dr3Entry>>::shard_of(&s, &make(u64::MAX / 4 + 1));
    assert!(next >= prev, "id-range buckets should be non-decreasing");
}

#[test]
fn excerpt_from_reader_streams_without_disk() {
    // Build the same fixture rows in memory and feed an in-memory `Cursor`
    // into `excerpt_csv_reader` — exercises the no-disk-input path that the
    // CLI uses for streamed CDN downloads.
    use starfield_gaia::excerpt::excerpt_csv_reader;
    use std::io::Cursor;

    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(HEADER.as_bytes());
    bytes.push(b'\n');
    for i in 0..50u64 {
        let id = i.wrapping_mul(0x123_4567_89AB_CDEF);
        let mag = 4.0 + (i as f64) * 0.05;
        let row = row_from(&HashMap::from([
            ("source_id", id.to_string().as_str()),
            ("solution_id", "1635721458409799680"),
            ("ref_epoch", "2016.0"),
            ("ra", "10.0"),
            ("ra_error", "0.04"),
            ("dec", "20.0"),
            ("dec_error", "0.04"),
            ("l", "0.0"),
            ("b", "0.0"),
            ("ecl_lon", "0.0"),
            ("ecl_lat", "0.0"),
            ("phot_g_mean_mag", mag.to_string().as_str()),
            ("phot_variable_flag", "NOT_AVAILABLE"),
        ]));
        bytes.extend_from_slice(row.as_bytes());
        bytes.push(b'\n');
    }
    let cursor: Box<dyn std::io::Read> = Box::new(Cursor::new(bytes));
    let out = tempfile::tempdir().unwrap();
    let summary = excerpt_csv_reader::<Dr3, _, _>(
        cursor,
        false, // not gzipped
        f64::INFINITY,
        out.path(),
        HashIdShard { num_shards: 4 },
        "synthetic.csv",
        |_| true,
    )
    .expect("excerpt from reader");
    assert_eq!(summary.input_rows, 50);
    assert_eq!(summary.kept_rows, 50);
}

#[test]
fn healpix_shard_returns_in_range() {
    let s = HealpixShard {
        num_shards: 32,
        level: 6,
    };
    let fixture = write_n_row_fixture(50);
    let out = tempfile::tempdir().unwrap();
    let summary =
        excerpt_csv_file::<Dr3, _, _>(fixture.path(), f64::INFINITY, out.path(), s, |_| true)
            .unwrap();
    assert_eq!(summary.kept_rows, 50);
    // Every shard count must be in 0..num_shards; sum equals kept.
    assert_eq!(summary.per_shard_counts.len(), 32);
    assert_eq!(summary.per_shard_counts.iter().sum::<u64>(), 50);
}

#[test]
fn resume_skips_already_processed_files_no_dups() {
    // Process 3 distinct fixtures into the same output dir across two writer
    // sessions. The second session must skip files already in the manifest
    // and end up with EXACTLY the union of rows, no duplicates.
    fn make_fixture(start_id: u64, n: u64) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "{}", HEADER).unwrap();
        for i in 0..n {
            let id = start_id + i;
            let row = row_from(&HashMap::from([
                ("source_id", id.to_string().as_str()),
                ("solution_id", "1635721458409799680"),
                ("ref_epoch", "2016.0"),
                ("ra", "10.0"),
                ("ra_error", "0.04"),
                ("dec", "20.0"),
                ("dec_error", "0.04"),
                ("l", "0.0"),
                ("b", "0.0"),
                ("ecl_lon", "0.0"),
                ("ecl_lat", "0.0"),
                ("phot_g_mean_mag", "10.0"),
                ("phot_variable_flag", "NOT_AVAILABLE"),
            ]));
            writeln!(f, "{}", row).unwrap();
        }
        f.flush().unwrap();
        f
    }

    let f1 = make_fixture(1_000_000, 60);
    let f2 = make_fixture(2_000_000, 80);
    let f3 = make_fixture(3_000_000, 100);
    let out = tempfile::tempdir().unwrap();

    // Session 1: process f1 + f2
    {
        let mut writer = ShardedCsvWriter::<Dr3, _>::new_or_resume(
            out.path(),
            HashIdShard { num_shards: 4 },
            f64::INFINITY,
        )
        .expect("open writer");
        excerpt_csv_file_into::<Dr3, _, _>(f1.path(), f64::INFINITY, &mut writer, |_| true)
            .expect("f1 first time");
        excerpt_csv_file_into::<Dr3, _, _>(f2.path(), f64::INFINITY, &mut writer, |_| true)
            .expect("f2 first time");
        let s = writer.finish().unwrap();
        assert_eq!(s.kept_rows, 140);
    }

    // Session 2 (simulates resume after a crash): pretend we don't know
    // what was processed, just iterate all 3 fixtures. Writer skips f1/f2,
    // processes f3.
    {
        let mut writer = ShardedCsvWriter::<Dr3, _>::new_or_resume(
            out.path(),
            HashIdShard { num_shards: 4 },
            f64::INFINITY,
        )
        .expect("re-open writer");
        let already = writer.processed_files().clone();
        assert!(already.contains(f1.path().file_name().unwrap().to_str().unwrap()));
        assert!(already.contains(f2.path().file_name().unwrap().to_str().unwrap()));

        for path in [f1.path(), f2.path(), f3.path()] {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            if already.contains(&name) {
                continue;
            }
            excerpt_csv_file_into::<Dr3, _, _>(path, f64::INFINITY, &mut writer, |_| true)
                .expect("f3 first time");
        }
        let s = writer.finish().unwrap();
        // Must equal the union, not double-count f1+f2.
        assert_eq!(s.kept_rows, 240, "resume double-counted or missed rows");
        assert_eq!(s.per_shard_counts.iter().sum::<u64>(), 240);
    }

    // Round-trip: reload every shard via Dr3Catalog and verify every
    // expected source_id is present exactly once.
    let mut all_ids = Vec::new();
    for shard in std::fs::read_dir(out.path()).unwrap() {
        let p = shard.unwrap().path();
        if !p.to_string_lossy().ends_with(".csv.gz") {
            continue;
        }
        let cat = Dr3Catalog::from_csv_file(&p, f64::INFINITY).expect("reload shard");
        for s in cat.stars() {
            all_ids.push(s.core.source_id);
        }
    }
    all_ids.sort();
    let mut expected: Vec<u64> = Vec::with_capacity(240);
    expected.extend(1_000_000..1_000_060);
    expected.extend(2_000_000..2_000_080);
    expected.extend(3_000_000..3_000_100);
    expected.sort();
    assert_eq!(all_ids, expected, "resume corrupted round-trip");
}

#[test]
fn manifest_validation_rejects_mismatched_resume() {
    // First run with 4 shards. Second run with 8 shards must refuse to attach.
    let fixture = write_n_row_fixture(20);
    let out = tempfile::tempdir().unwrap();
    excerpt_csv_file::<Dr3, _, _>(
        fixture.path(),
        f64::INFINITY,
        out.path(),
        HashIdShard { num_shards: 4 },
        |_| true,
    )
    .unwrap();

    let err = ShardedCsvWriter::<Dr3, _>::new_or_resume(
        out.path(),
        HashIdShard { num_shards: 8 }, // mismatch
        f64::INFINITY,
    )
    .err()
    .expect("must reject mismatched shard count");
    let msg = err.to_string();
    assert!(
        msg.contains("sharder mismatch") || msg.contains("shard count mismatch"),
        "unexpected error message: {}",
        msg
    );
}
