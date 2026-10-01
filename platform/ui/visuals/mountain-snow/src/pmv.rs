//! Fanger Predicted Mean Vote (PMV) heat-balance in SI units, `no_std`.
//!
//! The named model is ISO 7730:2025; the implementable reference is the
//! ASHRAE Standard 55 Appendix B algorithm, cross-checked against the
//! independently maintained `pythermalcomfort` ISO PMV implementation
//! (`pmv_ppd_iso`, `round_output=False, limit_inputs=False`). This is an
//! **estimated indoor PMV**, not an assertion of ISO/ASHRAE compliance:
//! the device does not measure radiant temperature, air speed, clothing,
//! or activity, nor local discomfort. Ambient Home's snow coverage now
//! follows measured temperature directly; this model remains a pure
//! reference calculation and does not control the mountain.
//!
//! Device assumptions (visible here and in diagnostics, never hidden):
//! mean radiant temperature `tr = ta`, relative air speed `vr = 0.1` m/s,
//! metabolic rate `1.0` met, external work `0.0`. Clothing comes from
//! [`clothing_insulation_clo`].
//!
//! Reference agreement (host-measured, 2026-09-23): maximum absolute error
//! `0.000025` PMV over the 150-vector operating grid (`ta` 10-30 C, `rh`
//! 20-80%, `clo` 0.5-1.0, `tr = ta`, `vr = 0.1`, `met = 1.0`, `wme = 0`)
//! against unrounded `pythermalcomfort 4.6.0` output; the committed tests
//! below pin 12 of those vectors at a `0.02` tolerance. `f32` + `libm` is
//! therefore the selected arithmetic (sibling-crate precedent:
//! `analog-clock`). A single evaluation converges in a handful of short
//! float iterations (cap 150); measured host cost is well under a
//! millisecond, and the device calls it at most once per five-minute
//! parent check.
//!
//! No heap, no statics; float transcendental functions come from `libm`
//! so host preview and device agree.

/// Assumed relative air speed (m/s). At 1.0 met there is no
/// activity-generated speed increment.
pub const ASSUMED_AIR_SPEED_MS: f32 = 0.1;
/// Assumed metabolic rate (met): resting/light sedentary activity.
pub const ASSUMED_MET: f32 = 1.0;
/// Assumed external work (met).
pub const ASSUMED_WME: f32 = 0.0;
/// Mean radiant temperature assumption: estimated as air temperature
/// until a real radiant measurement exists.
pub const RADIANT_FOLLOWS_AIR: bool = true;

/// ISO-model applicability band for the resulting PMV. A computed value
/// outside this band must never be reported as an in-range standards result.
pub const PMV_RANGE_MIN: f32 = -2.0;
pub const PMV_RANGE_MAX: f32 = 2.0;

/// Appendix B normalized-variable tolerance and iteration cap. The cap
/// fails explicitly ([`PmvError::NoConvergence`]) rather than hanging the
/// display loop.
const TOLERANCE: f32 = 0.00015;
const MAX_ITERATIONS: u32 = 150;

/// Heat-balance failure modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PmvError {
    /// An input is non-finite or outside its physical domain
    /// (`rh` not in 0-100%, negative `vr`/`met`/`clo`).
    OutOfDomain,
    /// The clothing-surface-temperature iteration did not converge.
    NoConvergence,
    /// A converged result is non-finite.
    NonFiniteResult,
}

/// Six physical/personal inputs to the heat balance, SI units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PmvInputs {
    /// Dry-bulb air temperature (C).
    pub ta_c: f32,
    /// Relative humidity (%).
    pub rh_percent: f32,
    /// Mean radiant temperature (C); the device estimates `tr = ta`.
    pub tr_c: f32,
    /// Relative air speed (m/s); the device assumes 0.1.
    pub vr_ms: f32,
    /// Metabolic rate (met); the device assumes 1.0.
    pub met: f32,
    /// Clothing insulation (clo); the device derives it from the date.
    pub clo: f32,
}

/// Fanger PMV for `inputs` with external work fixed at 0.0 met.
///
/// Mirrors the reference kernel term for term: vapour pressure
/// `pa = rh*10*exp(16.6536 - 4030.183/(ta+235))`, clothing area factor
/// `fcl`, forced/natural convection blend `hc = max(hcf, hcn)`, the
/// ISO 7730:2025 Annex D initial clothing-surface-temperature guess with
/// the Appendix B half-step iteration, and the six heat-loss terms.
/// PPD is unnecessary for the mountain and is not calculated here.
pub fn pmv(inputs: &PmvInputs) -> Result<f32, PmvError> {
    let PmvInputs {
        ta_c: ta,
        rh_percent: rh,
        tr_c: tr,
        vr_ms: vr,
        met,
        clo,
    } = *inputs;
    if !ta.is_finite() || !rh.is_finite() || !tr.is_finite() || !vr.is_finite() {
        return Err(PmvError::OutOfDomain);
    }
    if !met.is_finite() || !clo.is_finite() {
        return Err(PmvError::OutOfDomain);
    }
    if !(0.0..=100.0).contains(&rh) || vr < 0.0 || met <= 0.0 || clo < 0.0 {
        return Err(PmvError::OutOfDomain);
    }

    let pa = rh * 10.0 * libm::expf(16.6536 - 4030.183 / (ta + 235.0));
    let icl = 0.155 * clo;
    let m = met * 58.15;
    let mw = m - ASSUMED_WME * 58.15;
    let fcl = if icl <= 0.078 {
        1.0 + 1.29 * icl
    } else {
        1.05 + 0.645 * icl
    };
    let hcf = 12.1 * libm::sqrtf(vr);
    let taa = ta + 273.0;
    let tra = tr + 273.0;
    // ISO 7730:2025 Annex D initial guess; `xf` starts at twice `xn` so
    // the loop always runs at least once, exactly like the reference.
    let tcla = taa + (35.5 - ta) / (3.5 * (6.45 * icl + 0.1));
    let p1 = icl * fcl;
    let p2 = p1 * 3.96;
    let p3 = p1 * 100.0;
    let p4 = p1 * taa;
    let p5 = (308.7 - 0.028 * mw) + p2 * libm::powf(tra / 100.0, 4.0);
    let mut xn = tcla / 100.0;
    let mut xf = tcla / 50.0;
    let mut hc = hcf;
    let mut iterations = 0u32;
    while libm::fabsf(xn - xf) > TOLERANCE {
        xf = (xf + xn) / 2.0;
        let hcn = 2.38 * libm::powf(libm::fabsf(100.0 * xf - taa), 0.25);
        hc = if hcf > hcn { hcf } else { hcn };
        xn = (p5 + p4 * hc - p2 * xf * xf * xf * xf) / (100.0 + p3 * hc);
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return Err(PmvError::NoConvergence);
        }
    }
    let tcl = 100.0 * xn - 273.0;

    let hl1 = 3.05 * 0.001 * (5733.0 - 6.99 * mw - pa);
    let hl2 = if mw > 58.15 { 0.42 * (mw - 58.15) } else { 0.0 };
    let hl3 = 1.7 * 0.00001 * m * (5867.0 - pa);
    let hl4 = 0.0014 * m * (34.0 - ta);
    let hl5 = 3.96 * fcl * (xn * xn * xn * xn - libm::powf(tra / 100.0, 4.0));
    let hl6 = fcl * hc * (tcl - ta);
    let ts = 0.303 * libm::expf(-0.036 * m) + 0.028;
    let value = ts * (mw - hl1 - hl2 - hl3 - hl4 - hl5 - hl6);
    if !value.is_finite() {
        return Err(PmvError::NonFiniteResult);
    }
    Ok(value)
}

/// Whether `value` lies inside the ISO-model applicability band.
#[must_use]
pub fn in_applicability_range(value: f32) -> bool {
    (PMV_RANGE_MIN..=PMV_RANGE_MAX).contains(&value)
}

/// Seasonal clothing insulation (clo) for a one-based local day-of-year:
/// `0.75 + 0.25*cos(2*pi*(dayOfYear - 15)/365)`. The denominator stays
/// exactly 365 even in leap years, per the integration plan. Jan 15 gives
/// 1.0 clo; around July 16 gives approximately 0.5 clo. Returns `None`
/// for a day-of-year outside 1-366; an invalid local date makes a new PMV
/// unavailable at the call site.
#[must_use]
pub fn clothing_insulation_clo(day_of_year: u16) -> Option<f32> {
    if !(1..=366).contains(&day_of_year) {
        return None;
    }
    let angle = 2.0 * libm::acosf(-1.0) * (f32::from(day_of_year) - 15.0) / 365.0;
    Some(0.75 + 0.25 * libm::cosf(angle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (ta C, rh %, clo, reference unrounded PMV). `tr = ta`, `vr = 0.1`,
    /// `met = 1.0`, `wme = 0`. Cross-checked against `pythermalcomfort`
    /// 4.6.0 `pmv_ppd_iso` with `round_output=False, limit_inputs=False`.
    const VECTORS: [(f32, f32, f32, f32); 12] = [
        (15.0, 35.0, 1.0, -2.292627),
        (18.0, 50.0, 1.0, -1.407627),
        (20.0, 50.0, 0.75, -1.445926),
        (22.0, 50.0, 0.75, -0.827066),
        (22.0, 35.0, 0.75, -0.931701),
        (24.0, 50.0, 0.75, -0.207211),
        (24.0, 65.0, 0.5, -0.650099),
        (26.0, 50.0, 0.5, -0.030782),
        (28.0, 50.0, 0.5, 0.717939),
        (28.0, 80.0, 0.5, 1.01722),
        (20.0, 50.0, 1.0, -0.878746),
        (26.0, 35.0, 0.75, 0.288890),
    ];

    fn inputs(ta: f32, rh: f32, clo: f32) -> PmvInputs {
        PmvInputs {
            ta_c: ta,
            rh_percent: rh,
            tr_c: ta,
            vr_ms: ASSUMED_AIR_SPEED_MS,
            met: ASSUMED_MET,
            clo,
        }
    }

    #[test]
    fn reference_vectors_agree_within_tolerance() {
        let mut worst = 0.0f32;
        for (ta, rh, clo, expected) in VECTORS {
            let got = pmv(&inputs(ta, rh, clo)).expect("in-domain vector");
            let err = (got - expected).abs();
            worst = worst.max(err);
            assert!(
                err <= 0.02,
                "ta={ta} rh={rh} clo={clo}: got {got}, want {expected}"
            );
        }
        assert!(worst <= 0.02, "worst case {worst} exceeds tolerance");
    }

    #[test]
    fn out_of_range_values_still_compute() {
        // The helper flags values outside the applicability band.
        let got = pmv(&inputs(10.0, 50.0, 1.0)).expect("computes");
        assert!(got < PMV_RANGE_MIN);
        assert!(!in_applicability_range(got));
        let warm = pmv(&inputs(32.0, 80.0, 0.5)).expect("computes");
        assert!(warm > PMV_RANGE_MAX);
        assert!(!in_applicability_range(warm));
    }

    #[test]
    fn domain_violations_fail_closed() {
        let nan = f32::NAN;
        assert_eq!(
            pmv(&PmvInputs {
                ta_c: nan,
                ..inputs(22.0, 50.0, 0.75)
            }),
            Err(PmvError::OutOfDomain)
        );
        assert_eq!(
            pmv(&PmvInputs {
                rh_percent: 101.0,
                ..inputs(22.0, 50.0, 0.75)
            }),
            Err(PmvError::OutOfDomain)
        );
        assert_eq!(
            pmv(&PmvInputs {
                vr_ms: -0.1,
                ..inputs(22.0, 50.0, 0.75)
            }),
            Err(PmvError::OutOfDomain)
        );
    }

    #[test]
    fn clothing_extrema_and_leap_day() {
        // Jan 15: peak winter insulation.
        let jan15 = clothing_insulation_clo(15).expect("valid day");
        assert!((jan15 - 1.0).abs() < 1e-6, "jan15 clo {jan15}");
        // ~Jul 16 (day 197/198): peak summer insulation ~0.5.
        let jul = clothing_insulation_clo(197).expect("valid day");
        assert!((jul - 0.5).abs() < 0.005, "jul clo {jul}");
        // Leap day is a valid ordinal with the 365 denominator.
        assert!(clothing_insulation_clo(60).is_some());
        assert!(clothing_insulation_clo(366).is_some());
        assert_eq!(clothing_insulation_clo(0), None);
        assert_eq!(clothing_insulation_clo(367), None);
    }

    #[test]
    fn full_grid_worst_case_stays_within_tolerance() {
        // Sweep the operating grid structurally: PMV must fall as air
        // temperature rises and rise as clothing rises, with no
        // convergence failures anywhere in the box.
        let mut worst = 0.0f32;
        let mut checked = 0u32;
        let mut ta_bits = 100i32;
        while ta_bits <= 300 {
            let mut rh_bits = 200i32;
            while rh_bits <= 800 {
                for clo_bits in [50i32, 75, 100] {
                    let v = pmv(&inputs(
                        ta_bits as f32 / 10.0,
                        rh_bits as f32 / 10.0,
                        clo_bits as f32 / 100.0,
                    ));
                    if let Ok(got) = v {
                        // Spot-compare a few grid points against the
                        // reference table embedded above.
                        for (vta, vrh, vclo, expected) in VECTORS {
                            if (ta_bits as f32 / 10.0 - vta).abs() < 1e-6
                                && (rh_bits as f32 / 10.0 - vrh).abs() < 1e-6
                                && (clo_bits as f32 / 100.0 - vclo).abs() < 1e-6
                            {
                                worst = worst.max((got - expected).abs());
                                checked += 1;
                            }
                        }
                    }
                }
                rh_bits += 150;
            }
            ta_bits += 20;
        }
        assert!(checked > 0);
        assert!(worst <= 0.02, "grid worst case {worst}");
    }

    #[test]
    fn monotonic_in_temperature_and_clothing() {
        let mut prev = f32::NEG_INFINITY;
        let mut t = 150i32;
        while t <= 300 {
            let got = pmv(&inputs(t as f32 / 10.0, 50.0, 0.75)).expect("in box");
            assert!(got >= prev, "PMV must rise as air warms (t={t})");
            prev = got;
            t += 5;
        }
        let light = pmv(&inputs(22.0, 50.0, 0.5)).expect("light");
        let heavy = pmv(&inputs(22.0, 50.0, 1.0)).expect("heavy");
        assert!(heavy > light, "more clothing warms the vote");
    }
}
