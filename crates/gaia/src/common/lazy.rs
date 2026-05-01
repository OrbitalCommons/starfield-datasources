//! Lazy on-disk implementation of [`GaiaCatalog`].
//!
//! Opens an excerpt directory's manifest at construction (cheap; one
//! small JSON read), validates the layout matches the requested release
//! and is HEALPix-sharded one-file-per-cell, then re-reads the relevant
//! shards on every cone query. There is no in-memory cache — repeat
//! queries pay the parse cost again, deliberately, until a future
//! revision adds an LRU.

use std::marker::PhantomData;
use std::path::Path;

use starfield::{Result, StarfieldError};

use crate::common::catalog::GaiaCatalog;
use crate::common::cone::Cone;
use crate::common::reader::CsvSourceReader;
use crate::common::traits::{GaiaRelease, GaiaSource};
use crate::excerpt::ExcerptDir;

/// Lazy on-disk catalog backed by a HEALPix-sharded excerpt directory.
///
/// `open` checks the manifest is present, the recorded release matches
/// `R`, and the layout is one-file-per-cell — failures here surface
/// before the first query. Each `entries_in_cone` call uses
/// `cdshealpix::nested::cone_coverage_approx_flat` to identify the
/// covering cells, opens only the existing shard files for those cells,
/// streams entries through the magnitude gate, and post-filters by
/// great-circle distance against `cos(radius)` (the HEALPix covering is
/// conservative; boundary cells overshoot).
#[derive(Debug)]
pub struct LazyLoadingCatalog<R: GaiaRelease> {
    dir: ExcerptDir,
    healpix_level: u8,
    _marker: PhantomData<R>,
}

impl<R: GaiaRelease> LazyLoadingCatalog<R> {
    /// Open the excerpt dir and validate it can serve cone queries for
    /// release `R`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = ExcerptDir::open(dir)?;

        let want_release = format!("{:?}", R::RELEASE);
        if dir.manifest.release != want_release {
            return Err(StarfieldError::DataError(format!(
                "LazyLoadingCatalog: manifest at {} reports release {} but requested {}",
                dir.dir.display(),
                dir.manifest.release,
                want_release
            )));
        }

        let level = dir.healpix_level().ok_or_else(|| {
            StarfieldError::DataError(format!(
                "LazyLoadingCatalog requires a HEALPix-sharded directory; \
                 manifest at {} reports sharder kind={:?}",
                dir.dir.display(),
                dir.manifest.sharder.kind
            ))
        })?;

        // The post-PR-#42 writer maps shard index 1:1 to HEALPix cell, so
        // num_shards must equal `12 · 4^level`. Older mod-collapsed dirs
        // (kind="healpix" but num_shards < cell_count) would silently lose
        // ~99% of the cone's cells if we naively skipped out-of-range
        // indices, so refuse them up front.
        let expected_cells = 12u32 << (2 * level as u32);
        if dir.num_shards() != expected_cells {
            return Err(StarfieldError::DataError(format!(
                "LazyLoadingCatalog needs one-file-per-cell layout; manifest at {} reports \
                 HEALPix level={} but num_shards={} (expected {}). This dir was written by a \
                 mod-collapsed sharder; reshard with `gaia-excerpt --shard-by healpix \
                 --healpix-level {}` to get the cone-searchable layout.",
                dir.dir.display(),
                level,
                dir.num_shards(),
                expected_cells,
                level,
            )));
        }

        Ok(Self {
            dir,
            healpix_level: level,
            _marker: PhantomData,
        })
    }

    /// Underlying excerpt directory handle, for callers who want to read
    /// the manifest or enumerate shard paths directly.
    pub fn dir(&self) -> &ExcerptDir {
        &self.dir
    }

    /// HEALPix depth this directory is sharded at.
    pub fn healpix_level(&self) -> u8 {
        self.healpix_level
    }
}

impl<R: GaiaRelease> GaiaCatalog<R> for LazyLoadingCatalog<R> {
    /// Total committed row count from the manifest. No I/O — the writer
    /// keeps `kept_rows` in sync atomically on every commit.
    fn len(&self) -> u64 {
        self.dir.manifest.kept_rows
    }

    fn entries_in_cone<'a>(
        &'a self,
        cone: Cone,
        mag_limit: f64,
    ) -> Result<Box<dyn Iterator<Item = Result<R::Entry>> + 'a>> {
        let cells = cdshealpix::nested::cone_coverage_approx_flat(
            self.healpix_level,
            cone.ra_rad,
            cone.dec_rad,
            cone.radius_rad,
        );
        let cell_paths: Vec<_> = cells
            .iter()
            .filter_map(|&c| self.dir.existing_shard_path(c as u32))
            .collect();

        let it = cell_paths.into_iter().flat_map(move |path| {
            // `flat_map` needs a single iterator type per call. We open
            // the shard reader and adapt its `Result<Entry>` items;
            // open-time errors yield a single `Err` element so the
            // caller still sees them.
            let opened = CsvSourceReader::<R>::open(&path, mag_limit);
            let inner: Box<dyn Iterator<Item = Result<R::Entry>>> = match opened {
                Ok(reader) => Box::new(reader.filter_map(move |r| match r {
                    Ok(entry) => {
                        if cone.contains_unit_vec(&entry.core().unit_vector()) {
                            Some(Ok(entry))
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(Err(e)),
                })),
                Err(e) => Box::new(std::iter::once(Err(e))),
            };
            inner
        });

        Ok(Box::new(it))
    }
}
