//! Coupled spring-damper lattice and discrete bond momentum flux.

use crate::fixed::{clamp, mul, mul_round, QUARTER, SCALE};
use crate::topology::{ccw, chord, cw, STRANDS};

pub const K_SPIRAL: i64 = SCALE >> 2;
pub const K_CHORD: i64 = SCALE >> 3;
pub const K_ANCHOR: i64 = SCALE >> 1;
pub const GAMMA_MIN: i64 = (SCALE * 35) / 100;
pub const DT: i64 = SCALE >> 2;
const POS_LIMIT: i64 = 32 * SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MechReadout {
    pub x: [i64; STRANDS],
    pub v: [i64; STRANDS],
    pub tension: [i64; STRANDS],
    pub dissipated: [i64; STRANDS],
    pub total_energy: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluxReadout {
    pub div_j: [i64; STRANDS],
    pub flux_l1: i64,
    pub antisym_err: i64,
}

#[derive(Clone, Debug)]
pub struct HarmonicWeb {
    pub x: [i64; STRANDS],
    pub v: [i64; STRANDS],
}

impl Default for HarmonicWeb {
    fn default() -> Self {
        Self::new()
    }
}

impl HarmonicWeb {
    pub const fn new() -> Self {
        Self {
            x: [0i64; STRANDS],
            v: [0i64; STRANDS],
        }
    }

    pub fn step(
        &mut self,
        drive: &[i64; STRANDS],
        extra_damping: &[i64; STRANDS],
        connected: bool,
    ) -> MechReadout {
        let mut next_x = [0i64; STRANDS];
        let mut next_v = [0i64; STRANDS];
        let mut tension = [0i64; STRANDS];
        let mut dissipated = [0i64; STRANDS];

        for i in 0..STRANDS {
            let x_cw = self.x[cw(i, connected)];
            let x_ccw = self.x[ccw(i, connected)];
            let x_ch = self.x[chord(i, connected)];

            let spring = mul(K_SPIRAL, (x_cw - self.x[i]) + (x_ccw - self.x[i]))
                + mul(K_CHORD, x_ch - self.x[i])
                - mul(K_ANCHOR, self.x[i]);
            let gamma_eff = GAMMA_MIN + clamp(extra_damping[i], 0, QUARTER);
            let damp_force = mul(gamma_eff, self.v[i]);
            let accel = spring + clamp(drive[i], -16 * SCALE, 16 * SCALE) - damp_force;

            let v_new = clamp(self.v[i] + mul(DT, accel), -POS_LIMIT, POS_LIMIT);
            let x_new = clamp(self.x[i] + mul(DT, v_new), -POS_LIMIT, POS_LIMIT);

            // Viscous dissipation heat Q_diss = gamma_eff * v^2 * dt >= 0.
            let v_sq = mul(self.v[i], self.v[i]);
            dissipated[i] = mul(mul(gamma_eff, v_sq), DT).max(0);
            tension[i] = spring.abs();
            next_x[i] = x_new;
            next_v[i] = v_new;
        }
        self.x = next_x;
        self.v = next_v;

        let mut energy = 0i64;
        for i in 0..STRANDS {
            let ke = mul_round(self.v[i], self.v[i]) >> 1;
            let pe_anchor = mul_round(K_ANCHOR, mul_round(self.x[i], self.x[i])) >> 1;
            let mut pe_bonds = 0i64;
            if connected {
                let d_cw = self.x[cw(i, true)] - self.x[i];
                let d_ch = self.x[chord(i, true)] - self.x[i];
                pe_bonds += mul_round(K_SPIRAL, mul_round(d_cw, d_cw)) >> 1;
                pe_bonds += mul_round(K_CHORD, mul_round(d_ch, d_ch)) >> 2;
            }
            energy += ke + pe_anchor + pe_bonds;
        }

        MechReadout {
            x: self.x,
            v: self.v,
            tension,
            dissipated,
            total_energy: energy.max(0),
        }
    }
}

/// Computes discrete antisymmetric momentum/power flux on web bonds.
#[derive(Clone, Copy, Debug, Default)]
pub struct MomentumFlux;

impl MomentumFlux {
    pub fn measure(&self, web: &HarmonicWeb, connected: bool) -> FluxReadout {
        if !connected {
            return FluxReadout {
                div_j: [0; STRANDS],
                flux_l1: 0,
                antisym_err: 0,
            };
        }
        let mut div_j = [0i64; STRANDS];
        let mut flux_l1 = 0i64;
        let mut antisym_err = 0i64;

        for u in 0..STRANDS {
            let v_cw = cw(u, true);
            let j_uv = mul_round(K_SPIRAL, mul_round(web.x[u] - web.x[v_cw], (web.v[u] + web.v[v_cw]) >> 1));
            let j_vu = mul_round(K_SPIRAL, mul_round(web.x[v_cw] - web.x[u], (web.v[v_cw] + web.v[u]) >> 1));
            antisym_err = antisym_err.max((j_uv + j_vu).abs());
            div_j[u] += j_uv;
            div_j[v_cw] -= j_uv;
            flux_l1 += j_uv.abs();

            if u < (u ^ 8) {
                let v_ch = u ^ 8;
                let jc_uv = mul_round(K_CHORD, mul_round(web.x[u] - web.x[v_ch], (web.v[u] + web.v[v_ch]) >> 1));
                let jc_vu = mul_round(K_CHORD, mul_round(web.x[v_ch] - web.x[u], (web.v[v_ch] + web.v[u]) >> 1));
                antisym_err = antisym_err.max((jc_uv + jc_vu).abs());
                div_j[u] += jc_uv;
                div_j[v_ch] -= jc_uv;
                flux_l1 += jc_uv.abs();
            }
        }

        FluxReadout {
            div_j,
            flux_l1,
            antisym_err,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undriven_mechanical_decay_and_flux_antisymmetry() {
        let mut web = HarmonicWeb::new();
        let flux = MomentumFlux;
        let mut kick = [0i64; STRANDS];
        kick[3] = 2 * SCALE;
        let zero = [0i64; STRANDS];
        for _ in 0..4 {
            web.step(&kick, &zero, true);
        }
        let mut prev_e = web.step(&zero, &zero, true).total_energy;
        let start_e = prev_e;
        assert!(start_e > SCALE / 100);
        for _ in 0..150 {
            let r = web.step(&zero, &zero, true);
            assert!(r.total_energy <= prev_e, "energy rose {} -> {}", prev_e, r.total_energy);
            let fr = flux.measure(&web, true);
            assert_eq!(fr.antisym_err, 0);
            let div_sum: i64 = fr.div_j.iter().sum();
            assert_eq!(div_sum, 0);
            prev_e = r.total_energy;
        }
        assert!(prev_e < start_e / 50);
    }
}
