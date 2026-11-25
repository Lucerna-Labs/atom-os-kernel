//! Two mathematically distinct deterministic chaos mechanisms:
//! - `LogisticChaos` (dissipative 1D coupled map lattice across Feigenbaum onset)
//! - `StandardMapChaos` (conservative 2D symplectic Chirikov map across KAM breakup)

use crate::fixed::{clamp, div, mul, sin_cos, wrap_two_pi, QUARTER, SCALE, PI};
use crate::topology::{ccw, cw, STRANDS};

const U_MIN: i64 = SCALE / 1000;
const U_MAX: i64 = SCALE - U_MIN;
const R_BASE: i64 = (SCALE * 285) / 100;
const R_SPAN: i64 = (SCALE * 113) / 100;
const EPS_CML: i64 = SCALE >> 3;

const K_BASE: i64 = (SCALE * 45) / 100;
const K_SPAN: i64 = (SCALE * 175) / 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaosReadout {
    pub delta: [i64; STRANDS],
    pub spread: i64,
}

#[derive(Clone, Debug)]
pub struct LogisticChaos {
    pub u: [i64; STRANDS],
    pub u_ema: [i64; STRANDS],
}

impl Default for LogisticChaos {
    fn default() -> Self {
        Self::new()
    }
}

impl LogisticChaos {
    pub const fn new() -> Self {
        let mut init = [0i64; STRANDS];
        let mut i = 0;
        while i < STRANDS {
            init[i] = (SCALE * (230 + ((i as i64 * 37) % 540))) / 1000;
            i += 1;
        }
        Self {
            u: init,
            u_ema: init,
        }
    }

    pub fn step(&mut self, drive: &[i64; STRANDS], connected: bool) -> ChaosReadout {
        let mut f = [0i64; STRANDS];
        for v in 0..STRANDS {
            let d = clamp(drive[v].abs(), 0, 16 * SCALE);
            let sat = div(d, d + QUARTER);
            let r_v = R_BASE + mul(R_SPAN, sat);
            f[v] = mul(r_v, mul(self.u[v], SCALE - self.u[v]));
        }

        let mut delta = [0i64; STRANDS];
        let mut spread_sum = 0i64;
        for v in 0..STRANDS {
            let mixed = if connected {
                mul(SCALE - EPS_CML, f[v]) + mul(EPS_CML >> 1, f[cw(v, true)] + f[ccw(v, true)])
            } else {
                f[v]
            };
            self.u[v] = clamp(mixed, U_MIN, U_MAX);
            self.u_ema[v] = clamp(((self.u_ema[v] * 7) + self.u[v]) >> 3, U_MIN, U_MAX);
            let diff = self.u[v] - self.u_ema[v];
            delta[v] = diff >> 3;
            spread_sum += diff.abs();
        }

        ChaosReadout {
            delta,
            spread: spread_sum >> 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StandardMapChaos {
    pub p: [i64; STRANDS],
    pub theta: [i64; STRANDS],
    pub kick_ema: [i64; STRANDS],
}

impl Default for StandardMapChaos {
    fn default() -> Self {
        Self::new()
    }
}

impl StandardMapChaos {
    pub const fn new() -> Self {
        let mut p = [0i64; STRANDS];
        let mut theta = [0i64; STRANDS];
        let mut i = 0;
        let theta_center = (PI * 55) / 100;
        while i < STRANDS {
            let offset = (i as i64) - 8;
            p[i] = offset * (SCALE / 256);
            theta[i] = theta_center + offset * (SCALE / 128);
            i += 1;
        }
        Self {
            p,
            theta,
            kick_ema: [0i64; STRANDS],
        }
    }

    pub fn step(&mut self, flux_drive: &[i64; STRANDS]) -> ChaosReadout {
        let mut delta = [0i64; STRANDS];
        let mut spread_sum = 0i64;

        for v in 0..STRANDS {
            let d = clamp(flux_drive[v].abs(), 0, 16 * SCALE);
            let sat = div(d, d + QUARTER);
            let k_v = K_BASE + mul(K_SPAN, sat);

            let prev_theta = self.theta[v];
            let (sin_t, _) = sin_cos(prev_theta);
            let kick = mul(k_v, sin_t);
            let p_next = wrap_two_pi(self.p[v] + kick);
            let theta_next = wrap_two_pi(prev_theta + p_next);

            self.p[v] = p_next;
            self.theta[v] = theta_next;

            self.kick_ema[v] = ((self.kick_ema[v] * 15) + kick) >> 4;
            let centered = kick - self.kick_ema[v];
            delta[v] = centered >> 3;
            spread_sum += centered.abs();
        }

        ChaosReadout {
            delta,
            spread: spread_sum >> 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::TWO_PI;

    #[test]
    fn logistic_bifurcation_calm_low_chaotic_high() {
        let mut low = LogisticChaos::new();
        let mut high = LogisticChaos::new();
        let zero = [0i64; STRANDS];
        let big = [4 * SCALE; STRANDS];
        let mut low_spread = 0i64;
        let mut high_spread = 0i64;
        for _ in 0..120 {
            low_spread = low.step(&zero, true).spread;
            high_spread = high.step(&big, true).spread;
            for v in 0..STRANDS {
                assert!(low.u[v] >= U_MIN && low.u[v] <= U_MAX);
                assert!(high.u[v] >= U_MIN && high.u[v] <= U_MAX);
            }
        }
        assert!(low_spread < SCALE / 1000, "low_spread={low_spread}");
        assert!(high_spread > SCALE / 20, "high_spread={high_spread}");
    }

    #[test]
    fn standard_map_confinement_and_kam_breakup() {
        let mut calm = StandardMapChaos::new();
        let mut wild = StandardMapChaos::new();
        let zero = [0i64; STRANDS];
        let big = [4 * SCALE; STRANDS];
        let mut calm_acc = 0i64;
        let mut wild_acc = 0i64;
        for _ in 0..150 {
            let rc = calm.step(&zero);
            let rw = wild.step(&big);
            calm_acc += rc.spread;
            wild_acc += rw.spread;
            for v in 0..STRANDS {
                assert!(calm.p[v] >= 0 && calm.p[v] < TWO_PI);
                assert!(calm.theta[v] >= 0 && calm.theta[v] < TWO_PI);
                assert!(wild.p[v] >= 0 && wild.p[v] < TWO_PI);
                assert!(wild.theta[v] >= 0 && wild.theta[v] < TWO_PI);
            }
        }
        assert!(wild_acc > calm_acc, "calm={calm_acc}, wild={wild_acc}");
    }
}
