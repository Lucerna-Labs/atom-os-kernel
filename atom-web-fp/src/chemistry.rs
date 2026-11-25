//! Conserved 3-species reversible autocatalytic reaction network with
//! Arrhenius thermal/surprise gating and stoichiometric edge diffusion.

use crate::fixed::{clamp, div, exp_neg, mul, SCALE};
use crate::topology::{chord, cw, STRANDS};

pub const K_F0: i64 = SCALE >> 1;
pub const E_ACT: i64 = SCALE;
pub const K_R: i64 = SCALE >> 3;
pub const K_B: i64 = SCALE >> 4;
pub const K_INIT: i64 = SCALE >> 10;
pub const D_B: i64 = SCALE >> 5;

const A_INIT: i64 = (SCALE * 80) / 100;
const B_INIT: i64 = (SCALE * 10) / 100;
const C_INIT: i64 = SCALE - A_INIT - B_INIT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChemReadout {
    pub a: [i64; STRANDS],
    pub b: [i64; STRANDS],
    pub c: [i64; STRANDS],
    pub rxn_heat: [i64; STRANDS],
    pub b_total: i64,
    pub c_total: i64,
    pub max_mass_err: i64,
    pub min_species: i64,
}

#[derive(Clone, Debug)]
pub struct ReactionKinetics {
    pub a: [i64; STRANDS],
    pub b: [i64; STRANDS],
    pub c: [i64; STRANDS],
}

impl Default for ReactionKinetics {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactionKinetics {
    pub const fn new() -> Self {
        Self {
            a: [A_INIT; STRANDS],
            b: [B_INIT; STRANDS],
            c: [C_INIT; STRANDS],
        }
    }

    #[inline]
    fn diffuse_pair(&mut self, u: usize, v: usize) {
        let grad = mul(D_B, self.b[u] - self.b[v]);
        let flux = if grad >= 0 {
            grad.min(self.b[u] >> 2).min(self.a[v] >> 2)
        } else {
            -((-grad).min(self.b[v] >> 2).min(self.a[u] >> 2))
        };
        // Conservative exchange: (B_u, A_u) <-> (B_v, A_v) preserves (A+B+C) at every strand.
        self.b[u] -= flux;
        self.a[u] += flux;
        self.b[v] += flux;
        self.a[v] -= flux;
    }

    pub fn step(
        &mut self,
        temp: &[i64; STRANDS],
        surprise: &[i64; STRANDS],
        connected: bool,
    ) -> ChemReadout {
        let mut rxn_heat = [0i64; STRANDS];

        for v in 0..STRANDS {
            let t_eff = clamp(temp[v] + (surprise[v].max(0) >> 1), SCALE >> 2, 64 * SCALE);
            let k_f = mul(K_F0, exp_neg(div(E_ACT, t_eff)));

            let r0 = mul(K_INIT, self.a[v]).min(self.a[v] >> 2);
            let a_rem = self.a[v] - r0;
            let r1 = mul(k_f, mul(a_rem, self.b[v])).min(a_rem >> 1);
            let r2 = mul(K_R, self.b[v]).min(self.b[v] >> 1);
            let r3 = mul(K_B, self.c[v]).min(self.c[v] >> 1);

            self.a[v] = self.a[v] - r0 - r1 + r3;
            self.b[v] = self.b[v] + r0 + r1 - r2;
            self.c[v] = self.c[v] + r2 - r3;
            rxn_heat[v] = (r0 + r1) >> 2;
        }

        if connected {
            for u in 0..STRANDS {
                self.diffuse_pair(u, cw(u, true));
                if u < chord(u, true) {
                    self.diffuse_pair(u, chord(u, true));
                }
            }
        }

        let mut b_total = 0i64;
        let mut c_total = 0i64;
        let mut max_mass_err = 0i64;
        let mut min_species = i64::MAX;
        for v in 0..STRANDS {
            b_total += self.b[v];
            c_total += self.c[v];
            let err = (self.a[v] + self.b[v] + self.c[v] - SCALE).abs();
            if err > max_mass_err {
                max_mass_err = err;
            }
            min_species = min_species.min(self.a[v]).min(self.b[v]).min(self.c[v]);
        }

        ChemReadout {
            a: self.a,
            b: self.b,
            c: self.c,
            rxn_heat,
            b_total,
            c_total,
            max_mass_err,
            min_species,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stoichiometric_mass_conservation() {
        let mut chem = ReactionKinetics::new();
        let mut temp = [SCALE; STRANDS];
        let mut surp = [0i64; STRANDS];
        temp[4] = 4 * SCALE;
        surp[4] = 6 * SCALE;
        for _ in 0..200 {
            let r = chem.step(&temp, &surp, true);
            assert_eq!(r.max_mass_err, 0);
            assert!(r.min_species >= 0);
        }
    }
}
