//! Irreversible thermal bath, Fourier heat conduction, radiative cooling,
//! and thermodynamic entropy production.

use crate::fixed::{clamp, div, mul, QUARTER, SCALE};
use crate::topology::{ccw, chord, cw, STRANDS};

pub const T_AMBIENT: i64 = SCALE;
pub const T_MIN: i64 = QUARTER;
pub const T_MAX: i64 = 64 * SCALE;
pub const KAPPA_TH: i64 = SCALE >> 4;
pub const ALPHA_COOL: i64 = SCALE >> 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalReadout {
    pub temp: [i64; STRANDS],
    pub sigma: [i64; STRANDS],
    pub entropy_prod_total: i64,
    pub excess_temp_total: i64,
    pub min_temp: i64,
    pub min_entropy_prod: i64,
}

#[derive(Clone, Debug)]
pub struct ThermalBath {
    pub temp: [i64; STRANDS],
}

impl Default for ThermalBath {
    fn default() -> Self {
        Self::new()
    }
}

impl ThermalBath {
    pub const fn new() -> Self {
        Self {
            temp: [T_AMBIENT; STRANDS],
        }
    }

    pub fn step(&mut self, heat_in: &[i64; STRANDS], connected: bool) -> ThermalReadout {
        let mut next_temp = [T_AMBIENT; STRANDS];
        let mut sigma = [0i64; STRANDS];
        let mut entropy_prod_total = 0i64;
        let mut min_entropy_prod = i64::MAX;

        for i in 0..STRANDS {
            let q = clamp(heat_in[i], 0, 16 * SCALE);
            let s_i = div(q, self.temp[i]).max(0);
            sigma[i] = s_i;
            entropy_prod_total += s_i;
            if s_i < min_entropy_prod {
                min_entropy_prod = s_i;
            }

            let cond = if connected {
                let d_cw = self.temp[cw(i, true)] - self.temp[i];
                let d_ccw = self.temp[ccw(i, true)] - self.temp[i];
                let d_ch = self.temp[chord(i, true)] - self.temp[i];
                mul(KAPPA_TH, d_cw + d_ccw + d_ch)
            } else {
                0
            };
            let cool = mul(ALPHA_COOL, T_AMBIENT - self.temp[i]);
            next_temp[i] = clamp(self.temp[i] + q + cond + cool, T_MIN, T_MAX);
        }
        self.temp = next_temp;

        let mut excess_temp_total = 0i64;
        let mut min_temp = i64::MAX;
        for i in 0..STRANDS {
            excess_temp_total += (self.temp[i] - T_AMBIENT).abs();
            if self.temp[i] < min_temp {
                min_temp = self.temp[i];
            }
        }

        ThermalReadout {
            temp: self.temp,
            sigma,
            entropy_prod_total,
            excess_temp_total,
            min_temp,
            min_entropy_prod,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_law_and_exponential_cooling() {
        let mut bath = ThermalBath::new();
        let mut heat = [0i64; STRANDS];
        heat[2] = SCALE;
        let r1 = bath.step(&heat, true);
        assert!(r1.min_entropy_prod >= 0);
        assert!(r1.entropy_prod_total > 0);
        assert!(r1.temp[2] > T_AMBIENT);

        let zero = [0i64; STRANDS];
        let mut prev_excess = r1.excess_temp_total;
        for _ in 0..128 {
            let r = bath.step(&zero, true);
            assert!(r.min_temp >= T_MIN);
            assert!(r.min_entropy_prod >= 0);
            prev_excess = r.excess_temp_total;
        }
        assert!(prev_excess < r1.excess_temp_total / 50);
    }
}
