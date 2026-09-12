//! Emit a server allow-list from the SAME artifact constructors used by clients.
//! Does not download artifact bodies. Gaia checksum lists and MAST product JSON
//! are explicit inputs so an operator reviews the selected catalog and license.

use starfield_datastore::Manifest;
use starfield_gaia::{Downloader, Dr1, Dr2, Dr3, GaiaRelease};
use starfield_mast::DataProduct;
use starfield_planet_maps::PRODUCTS;
use starfield_planet_spectra::Product;

fn gaia<R: GaiaRelease>(
    manifest: &mut Manifest,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    manifest
        .artifacts
        .push(Downloader::<R>::artifact(R::MD5_FILENAME)?);
    let text = std::fs::read_to_string(path)?;
    let mut count = 0;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2
            || fields[0].len() != 32
            || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid Gaia checksum list".into());
        }
        let name = fields[1].trim_start_matches('*');
        if !name.ends_with(".csv.gz") {
            continue;
        }
        manifest.artifacts.push(Downloader::<R>::artifact(name)?);
        count += 1;
    }
    if count == 0 {
        return Err("Gaia checksum list contains no CSV.gz artifacts".into());
    }
    eprintln!(
        "registered {} {count} shards (no artifact downloads)",
        R::RELEASE.as_str()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = Manifest::default();
    for product in PRODUCTS {
        manifest.artifacts.push(product.artifact(None)?);
    }
    for product in [Product::Low1995, Product::High1995, Product::Table1993] {
        manifest.artifacts.push(product.artifact());
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base" => {
                let path = args.next().ok_or("--base needs a TOML path")?;
                let base = Manifest::from_path(std::path::Path::new(&path))?;
                manifest.artifacts.extend(base.artifacts);
            }
            "--gaia" => {
                let release = args.next().ok_or("--gaia needs dr1|dr2|dr3 and checksum path")?;
                let path = args.next().ok_or("--gaia needs a checksum path")?;
                match release.as_str() {
                    "dr1" => gaia::<Dr1>(&mut manifest, &path)?,
                    "dr2" => gaia::<Dr2>(&mut manifest, &path)?,
                    "dr3" => gaia::<Dr3>(&mut manifest, &path)?,
                    _ => return Err("unknown Gaia release".into()),
                }
            }
            "--mast" => {
                let path = args.next().ok_or("--mast needs product JSON and a license")?;
                let license = args.next().ok_or("--mast needs an explicit redistribution license")?;
                if license.trim().is_empty() { return Err("MAST license cannot be blank".into()); }
                let products: Vec<DataProduct> = serde_json::from_reader(std::fs::File::open(path)?)?;
                for product in products {
                    let mut artifact = product.artifact()?;
                    artifact.provenance.license = license.clone();
                    manifest.artifacts.push(artifact);
                }
            }
            _ => return Err("usage: datastore_manifest [--base kernels.toml] [--gaia dr1|dr2|dr3 MD5SUM.txt] [--mast products.json LICENSE] > serve.toml".into()),
        }
    }
    print!("{}", manifest.to_toml_string()?);
    Ok(())
}
