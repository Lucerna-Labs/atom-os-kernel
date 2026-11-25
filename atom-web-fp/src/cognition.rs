//! Two-tap precision-weighted predictive coding & habituation cognition unit.

use crate::fixed::{clamp, div, mul, QUARTER, SCALE};
use crate::topology::{ccw, chord, cw, STRANDS};

pub const PRECISION_MIN: i64 = QUARTER;
pub const PRECISION_MAX: i64 = 4 * SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CogReadout {
    pub fe: [i64; STRANDS],
    pub damp_mod: [i64; STRANDS],
    pub free_energy_mean: i64,
    pub precision_mean: i64,
}

#[derive(Clone, Debug)]
pub struct PredictiveCognition {
    pub pred: [i64; STRANDS],
    pub trend: [i64; STRANDS],
    pub precision: [i64; STRANDS],
    pub err_var: [i64; STRANDS],
}

impl Default for PredictiveCognition {
    fn default() -> Self {
        Self::new()
    }
}

impl PredictiveCognition {
    pub const fn new() -> Self {
        Self {
            pred: [0i64; STRANDS],
            trend: [0i64; STRANDS],
            precision: [SCALE; STRANDS],
            err_var: [SCALE >> 4; STRANDS],
        }
    }

    pub fn step(&mut self, obs_y: &[i64; STRANDS], connected: bool) -> CogReadout {
        let mut next_pred = [0i64; STRANDS];
        let mut next_trend = [0i64; STRANDS];
        let mut fe = [0i64; STRANDS];
        let mut damp_mod = [0i64; STRANDS];
        let mut fe_sum = 0i64;
        let mut prec_sum = 0i64;

        for v in 0..STRANDS {
            let obs = clamp(obs_y[v], -64 * SCALE, 64 * SCALE);
            let neighbor_adj = if connected {
                (self.pred[cw(v, true)] + self.pred[ccw(v, true)] + self.pred[chord(v, true)]
                    - 3 * self.pred[v])
                    >> 4
            } else {
                0
            };
            let y_hat = self.pred[v] + (self.trend[v] >> 2) + neighbor_adj;
            let err = obs - y_hat;
            let abs_err = err.abs();

            let fe_v = mul(self.precision[v], abs_err);
            fe[v] = fe_v;
            damp_mod[v] = clamp(fe_v >> 3, 0, QUARTER);
            fe_sum += fe_v;

            let err_sq = mul(err, err);
            self.err_var[v] = clamp(
                ((self.err_var[v] * 15) >> 4) + (err_sq >> 4),
                0,
                64 * SCALE,
            );
            self.precision[v] = clamp(
                div(SCALE >> 3, self.err_var[v] + (SCALE >> 5)),
                PRECISION_MIN,
                PRECISION_MAX,
            );
            prec_sum += self.precision[v];

            next_pred[v] = clamp(self.pred[v] + (err >> 3), -64 * SCALE, 64 * SCALE);
            next_trend[v] = clamp(((self.trend[v] * 7) >> 3) + (err >> 3), -16 * SCALE, 16 * SCALE);
        }

        self.pred = next_pred;
        self.trend = next_trend;

        CogReadout {
            fe,
            damp_mod,
            free_energy_mean: fe_sum >> 4,
            precision_mean: prec_sum >> 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn habituation_and_bounded_precision() {
        let mut cog = PredictiveCognition::new();
        let obs = [SCALE; STRANDS];
        let mut first_fe = 0i64;
        let mut last_fe = 0i64;
        for t in 0..80 {
            let r = cog.step(&obs, true);
            assert!(cog.precision[0] >= PRECISION_MIN && cog.precision[0] <= PRECISION_MAX);
            if t == 0 {
                first_fe = r.free_energy_mean;
            }
            last_fe = r.free_energy_mean;
        }
        assert!(last_fe < first_fe / 20, "first={first_fe}, last={last_fe}");
    }
}
