//! A wavelength-sampled curve with consistent band-mean semantics.
//!
//! Several datasource crates publish the same shape of thing: a quantity
//! tabulated against wavelength, which consumers need to evaluate at a point or
//! average over a detector band. Spectral albedos, surface reflectances and
//! solar irradiance all qualify. Sharing one implementation keeps their
//! band-mean semantics identical, which matters because a renderer combines
//! values from all three in a single expression — if they disagreed about how a
//! partially covered band is handled, the discrepancy would appear as a
//! photometric error with no obvious cause.

use starfield::{Result, StarfieldError};

/// A quantity sampled against wavelength, treated as piecewise linear between
/// samples.
///
/// Wavelengths are in nanometres and strictly ascending. Both [`SampledCurve::at`]
/// and [`SampledCurve::mean_over`] are exact for the piecewise-linear reading.
///
/// The type refuses to extrapolate beyond its range, and refuses to average a
/// band it does not fully cover. Both would otherwise return a plausible number
/// that silently understates how much of the band was missing.
#[derive(Debug, Clone, PartialEq)]
pub struct SampledCurve {
    wavelengths_nm: Vec<f64>,
    values: Vec<f64>,
}

impl SampledCurve {
    /// Build from parallel arrays, validating the wavelength grid.
    ///
    /// Returns an error rather than panicking, because this is the entry point
    /// for parsing external archive files whose contents are not under our
    /// control.
    pub fn new(wavelengths_nm: Vec<f64>, values: Vec<f64>) -> Result<Self> {
        if wavelengths_nm.len() != values.len() {
            return Err(StarfieldError::DataError(format!(
                "sampled curve: {} wavelengths but {} values",
                wavelengths_nm.len(),
                values.len()
            )));
        }
        if wavelengths_nm.is_empty() {
            return Err(StarfieldError::DataError(
                "sampled curve must have at least one sample".into(),
            ));
        }
        // Strictly ascending means every adjacent pair compares as Less. Equal,
        // Greater and None (a NaN in the grid) are all rejected; `>=` alone
        // would silently accept NaN, since every NaN comparison is false.
        let ascending = |w: &[f64]| w[0].partial_cmp(&w[1]) == Some(std::cmp::Ordering::Less);
        if let Some(i) = wavelengths_nm.windows(2).position(|w| !ascending(w)) {
            return Err(StarfieldError::DataError(format!(
                "sampled curve wavelengths must be strictly ascending: {} nm then {} nm at index {i}",
                wavelengths_nm[i],
                wavelengths_nm[i + 1]
            )));
        }
        Ok(Self {
            wavelengths_nm,
            values,
        })
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.wavelengths_nm.len()
    }

    /// Always false — construction rejects empty curves.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Inclusive wavelength bounds, nm.
    pub fn range_nm(&self) -> (f64, f64) {
        (
            self.wavelengths_nm[0],
            self.wavelengths_nm[self.wavelengths_nm.len() - 1],
        )
    }

    /// The samples, as `(wavelength_nm, value)` pairs.
    pub fn samples(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.wavelengths_nm
            .iter()
            .copied()
            .zip(self.values.iter().copied())
    }

    /// Sample wavelengths, nm.
    pub fn wavelengths_nm(&self) -> &[f64] {
        &self.wavelengths_nm
    }

    /// Sample values.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Value at `nm`, linearly interpolated. `None` outside [`SampledCurve::range_nm`].
    pub fn at(&self, nm: f64) -> Option<f64> {
        let (lo, hi) = self.range_nm();
        if !(lo..=hi).contains(&nm) {
            return None;
        }
        let idx = self.wavelengths_nm.partition_point(|&w| w < nm);
        if self.wavelengths_nm[idx] == nm {
            return Some(self.values[idx]);
        }
        let (w0, w1) = (self.wavelengths_nm[idx - 1], self.wavelengths_nm[idx]);
        let (v0, v1) = (self.values[idx - 1], self.values[idx]);
        Some(v0 + (nm - w0) / (w1 - w0) * (v1 - v0))
    }

    /// Mean value over `[lo_nm, hi_nm]`, weighted by wavelength.
    ///
    /// `None` unless the interval is valid and wholly inside
    /// [`SampledCurve::range_nm`].
    pub fn mean_over(&self, lo_nm: f64, hi_nm: f64) -> Option<f64> {
        self.integrate(lo_nm, hi_nm)
            .map(|total| total / (hi_nm - lo_nm))
    }

    /// Integral over `[lo_nm, hi_nm]`, in value·nm.
    pub fn integrate(&self, lo_nm: f64, hi_nm: f64) -> Option<f64> {
        if !lo_nm.is_finite() || !hi_nm.is_finite() || lo_nm >= hi_nm {
            return None;
        }
        let (min, max) = self.range_nm();
        if lo_nm < min || hi_nm > max {
            return None;
        }
        let mut total = 0.0;
        for i in 0..self.wavelengths_nm.len() - 1 {
            let (w0, w1) = (self.wavelengths_nm[i], self.wavelengths_nm[i + 1]);
            let a = w0.max(lo_nm);
            let b = w1.min(hi_nm);
            if a >= b {
                continue;
            }
            // Exact for a linear segment.
            total += 0.5 * (self.at(a)? + self.at(b)?) * (b - a);
        }
        Some(total)
    }

    /// Restrict to `[lo_nm, hi_nm]`, keeping interior samples and interpolating
    /// exact endpoints.
    ///
    /// Useful for trimming an archive that runs far past a detector's response
    /// before embedding it.
    pub fn clipped(&self, lo_nm: f64, hi_nm: f64) -> Option<SampledCurve> {
        let lo_value = self.at(lo_nm)?;
        let hi_value = self.at(hi_nm)?;
        if lo_nm >= hi_nm {
            return None;
        }
        let mut wavelengths = vec![lo_nm];
        let mut values = vec![lo_value];
        for (w, v) in self.samples() {
            if w > lo_nm && w < hi_nm {
                wavelengths.push(w);
                values.push(v);
            }
        }
        wavelengths.push(hi_nm);
        values.push(hi_value);
        SampledCurve::new(wavelengths, values).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> SampledCurve {
        SampledCurve::new(vec![400.0, 500.0, 600.0], vec![0.4, 0.5, 0.6]).unwrap()
    }

    #[test]
    fn interpolates_linearly_and_hits_samples() {
        let c = ramp();
        assert_eq!(c.at(400.0), Some(0.4));
        assert_eq!(c.at(600.0), Some(0.6));
        assert!((c.at(450.0).unwrap() - 0.45).abs() < 1e-12);
    }

    #[test]
    fn refuses_to_extrapolate() {
        let c = ramp();
        assert_eq!(c.at(399.9), None);
        assert_eq!(c.at(600.1), None);
        assert_eq!(c.at(f64::NAN), None);
    }

    #[test]
    fn mean_of_a_linear_ramp_is_the_midpoint() {
        let c = ramp();
        assert!((c.mean_over(400.0, 600.0).unwrap() - 0.5).abs() < 1e-12);
        assert!((c.mean_over(410.0, 430.0).unwrap() - 0.42).abs() < 1e-12);
    }

    #[test]
    fn rejects_partial_coverage_and_degenerate_intervals() {
        let c = ramp();
        assert_eq!(c.mean_over(350.0, 500.0), None);
        assert_eq!(c.mean_over(500.0, 700.0), None);
        assert_eq!(c.mean_over(500.0, 500.0), None);
        assert_eq!(c.mean_over(550.0, 450.0), None);
        assert_eq!(c.mean_over(f64::NAN, 500.0), None);
    }

    #[test]
    fn sub_band_integrals_sum_to_the_whole() {
        let c = ramp();
        let whole = c.integrate(400.0, 600.0).unwrap();
        let parts = c.integrate(400.0, 437.0).unwrap() + c.integrate(437.0, 600.0).unwrap();
        assert!((whole - parts).abs() < 1e-9);
    }

    #[test]
    fn construction_validates_the_grid() {
        assert!(SampledCurve::new(vec![500.0, 400.0], vec![0.5, 0.4]).is_err());
        assert!(SampledCurve::new(vec![400.0, 400.0], vec![0.5, 0.4]).is_err());
        assert!(SampledCurve::new(vec![400.0, 500.0], vec![0.4]).is_err());
        assert!(SampledCurve::new(vec![], vec![]).is_err());
        // A NaN in the grid must be rejected, not silently accepted: every
        // comparison against NaN is false, so a plain `>=` check would pass it.
        assert!(SampledCurve::new(vec![400.0, f64::NAN, 600.0], vec![0.4, 0.5, 0.6]).is_err());
    }

    #[test]
    fn clipping_keeps_endpoints_exact() {
        let c = ramp();
        let clipped = c.clipped(450.0, 550.0).unwrap();
        assert_eq!(clipped.range_nm(), (450.0, 550.0));
        assert!((clipped.at(450.0).unwrap() - 0.45).abs() < 1e-12);
        assert!((clipped.at(550.0).unwrap() - 0.55).abs() < 1e-12);
        // The interior sample at 500 nm survives.
        assert_eq!(clipped.len(), 3);
        // And the mean is unchanged from evaluating the original over the range.
        assert!(
            (clipped.mean_over(450.0, 550.0).unwrap() - c.mean_over(450.0, 550.0).unwrap()).abs()
                < 1e-12
        );
    }

    #[test]
    fn clipping_outside_the_range_fails() {
        let c = ramp();
        assert!(c.clipped(300.0, 500.0).is_none());
        assert!(c.clipped(500.0, 700.0).is_none());
    }
}
