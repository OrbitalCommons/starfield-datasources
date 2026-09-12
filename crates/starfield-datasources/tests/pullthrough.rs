#[path = "support/stub.rs"]
mod stub;

use starfield_datasource_utils::{adopt_legacy, artifact::materialize, sha256_bytes};
use starfield_datastore::{Datastore, Mirror};
use starfield_gaia::{Downloader, Dr1, Dr2, Dr3};
use starfield_mast::{DataProduct, MastClient};
use starfield_planet_maps::PRODUCTS;
use stub::{Response, Stub};

fn store(root: &std::path::Path, mirror: &Stub) -> Datastore {
    Datastore::builder()
        .cache_root(root.to_path_buf())
        .mirror(Mirror::Http {
            base_url: mirror.url(),
            writable: false,
        })
        .allow_upstream(false)
        .build()
        .unwrap()
}

#[test]
fn map_uses_mirror_key_rejects_html_and_honors_digest_on_hits() {
    let mirror = Stub::start("127.0.0.1");
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path(), &mirror);
    let product = &PRODUCTS[0];
    let artifact = product.artifact(None).unwrap();
    let route = format!("/artifact/{}", artifact.key);
    mirror.route(&route, Response::ok(b"<html>login</html>".to_vec()));
    assert!(product.download_with(&store, None).is_err());
    assert!(!store.contains(&artifact.key));

    let mut bytes = vec![0u8; 2048];
    bytes[..4].copy_from_slice(b"II+\0");
    mirror.route(&route, Response::ok(bytes.clone()));
    let digest = sha256_bytes(&bytes);
    let path = product.download_with(&store, Some(&digest)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(product
        .download_with(&store, Some(&"0".repeat(64)))
        .is_err());
    assert_eq!(product.download_with(&store, Some(&digest)).unwrap(), path);
    let requests = mirror.requests();
    // A conflicting pin rejects the local hit and retries the mirror before
    // reporting the rejection; the subsequent correct pin is a local hit.
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .all(|r| r.path == route && r.method == "GET" && !r.headers.contains_key("authorization")));
}

#[test]
fn explicit_legacy_adoption_is_validated_and_alias_cleanup_preserves_blob() {
    let root = tempfile::tempdir().unwrap();
    let store = Datastore::builder()
        .cache_root(root.path().join("cache"))
        .offline(true)
        .build()
        .unwrap();
    let product = &PRODUCTS[0];
    let artifact = product.artifact(None).unwrap();
    let legacy = root.path().join(product.file_name);
    std::fs::write(&legacy, b"<html>bad legacy</html>").unwrap();
    adopt_legacy(&store, &artifact, &legacy).unwrap();
    assert!(!store.contains(&artifact.key));
    let mut bytes = vec![0u8; 2048];
    bytes[..4].copy_from_slice(b"II*\0");
    std::fs::write(&legacy, &bytes).unwrap();
    // A caller-supplied offline store never discovers the legacy file itself.
    assert!(product.download_with(&store, None).is_err());
    adopt_legacy(&store, &artifact, &legacy).unwrap();
    let blob = product.download_with(&store, None).unwrap();
    let alias = root.path().join("alias.tif");
    materialize(&blob, &alias).unwrap();
    std::fs::remove_file(alias).unwrap();
    assert_eq!(std::fs::read(blob).unwrap(), bytes);
}

#[test]
fn gaia_checksums_and_shards_both_use_mirror_and_bad_md5_is_evicted() {
    let mirror = Stub::start("127.0.0.1");
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path(), &mirror);
    let filename = "GaiaSource_test.csv.gz";
    let artifact = Downloader::<Dr3>::artifact(filename).unwrap();
    let mut bytes = vec![0u8; 2048];
    bytes[..2].copy_from_slice(&[0x1f, 0x8b]);
    let checksum = format!("{:x}  {filename}\n", md5::compute(&bytes));
    mirror.route(
        "/artifact/gaia/dr3/gaia_source/_MD5SUM.txt",
        Response::ok(checksum),
    );
    mirror.route(
        &format!("/artifact/{}", artifact.key),
        Response::ok(bytes.clone()),
    );
    let path = Downloader::<Dr3>::download_file_with(&store, filename).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(mirror.requests().len(), 2);
    store.remove(&artifact.key).unwrap();
    bytes[20] = 1;
    mirror.route(&format!("/artifact/{}", artifact.key), Response::ok(bytes));
    assert!(Downloader::<Dr3>::download_file_with(&store, filename).is_err());
    assert!(!store.contains(&artifact.key));
    assert_eq!(
        Downloader::<Dr1>::artifact("MD5SUM.txt")
            .unwrap()
            .key
            .as_str(),
        "gaia/dr1/gaia_source/MD5SUM.txt"
    );
    assert_eq!(
        Downloader::<Dr2>::artifact("MD5SUM.txt")
            .unwrap()
            .key
            .as_str(),
        "gaia/dr2/gaia_source/MD5SUM.txt"
    );
    assert!(Downloader::<Dr3>::artifact("../escape.gz").is_err());
}

#[test]
fn mast_product_identity_and_mirror_fit_validation() {
    let mirror = Stub::start("127.0.0.1");
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path(), &mirror);
    let product: DataProduct = serde_json::from_value(serde_json::json!({
        "productFilename": "test.fits", "dataURI": "mast:HST/product/test.fits", "size": 2880
    }))
    .unwrap();
    let artifact = product.artifact().unwrap();
    assert_eq!(artifact.key.as_str(), "mast/HST/product/test.fits");
    let mut bytes = vec![b' '; 2880];
    bytes[..9].copy_from_slice(b"SIMPLE  =");
    mirror.route(
        &format!("/artifact/{}", artifact.key),
        Response::ok(bytes.clone()),
    );
    let client = MastClient::new().unwrap();
    let path = client.download_product_with(&store, &product).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(mirror.requests().len(), 1);
}
