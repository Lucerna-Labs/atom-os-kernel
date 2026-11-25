//! Quantum-derived base mechanics on the 16-strand, 4-arc spiderweb graph
//! (plus classical Markov walk replacement control).

use crate::fixed::{clamp, div, mul_round, sin_cos, ComplexQ30, HALF, HALF_PI, QUARTER, SCALE};
use crate::topology::{ccw, chord, cw, ARCS, STRANDS};

pub const UNIFORM_PROB: i64 = SCALE / (STRANDS as i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantumReadout {
    pub probs: [i64; STRANDS],
    pub contrast: [i64; STRANDS],
    pub born_l1: i64,
    pub contrast_defect: i64,
    pub norm_err: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterferometerReadout {
    pub probs: [i64; STRANDS],
    pub contrast: [i64; STRANDS],
    pub chiral: [i64; STRANDS],
    pub chiral_total: i64,
    pub born_l1: i64,
    pub contrast_defect: i64,
    pub norm_err: i64,
}

/// Coined discrete-time quantum walk on the 16-node, 4-arc spiderweb graph.
#[derive(Clone, Debug)]
pub struct CoinedWebWalk {
    pub amp: [[ComplexQ30; ARCS]; STRANDS],
}

impl Default for CoinedWebWalk {
    fn default() -> Self {
        Self::new()
    }
}

impl CoinedWebWalk {
    pub const fn new() -> Self {
        Self {
            amp: [[ComplexQ30::new(SCALE >> 3, 0); ARCS]; STRANDS],
        }
    }

    pub fn contrast(&self) -> [i64; STRANDS] {
        let mut out = [0i64; STRANDS];
        for v in 0..STRANDS {
            let mut sum = ComplexQ30::default();
            let mut prob = 0i64;
            for d in 0..ARCS {
                sum = sum.add(self.amp[v][d]);
                prob += self.amp[v][d].norm_sq();
            }
            let coh_sq = sum.norm_sq();
            out[v] = div(coh_sq, (prob << 2) + 4096).min(SCALE);
        }
        out
    }

    pub fn probs(&self) -> ([i64; STRANDS], i64) {
        let mut probs = [0i64; STRANDS];
        let mut total = 0i64;
        for v in 0..STRANDS {
            let mut p = 0i64;
            for d in 0..ARCS {
                p += self.amp[v][d].norm_sq();
            }
            probs[v] = p;
            total += p;
        }
        (probs, (total - SCALE).abs())
    }

    pub fn step(
        &mut self,
        phi_spiral: &[i64; STRANDS],
        phi_chord: &[i64; STRANDS],
        phi_anchor: &[i64; STRANDS],
        connected: bool,
    ) -> QuantumReadout {
        let mut next = [[ComplexQ30::default(); ARCS]; STRANDS];
        for v in 0..STRANDS {
            let (ss, cs) = sin_cos(phi_spiral[v]);
            let (sc, cc) = sin_cos(phi_chord[v]);
            let (sa, ca) = sin_cos(phi_anchor[v]);

            let p0 = self.amp[v][0].mul_unit(cs, ss);
            let p1 = self.amp[v][1].mul_unit(cs, -ss);
            let p2 = self.amp[v][2].mul_unit(cc, sc);
            let p3 = self.amp[v][3].mul_unit(ca, sa);

            // Exact 4x4 Grover diffusion coin: c_d = 0.5 * sum_k p_k - p_d.
            let sum = p0.add(p1).add(p2).add(p3);
            let half_sum = ComplexQ30::new(sum.re >> 1, sum.im >> 1);
            let c0 = half_sum.sub(p0);
            let c1 = half_sum.sub(p1);
            let c2 = half_sum.sub(p2);
            let c3 = half_sum.sub(p3);

            // Involutive edge shift S.
            next[cw(v, connected)][1] = c0;
            next[ccw(v, connected)][0] = c1;
            next[chord(v, connected)][2] = c2;
            next[v][3] = c3;
        }
        self.amp = next;

        let (probs, norm_err) = self.probs();
        let contrast = self.contrast();
        let mut born_l1 = 0i64;
        let mut contrast_defect = 0i64;
        for v in 0..STRANDS {
            born_l1 += (probs[v] - UNIFORM_PROB).abs();
            contrast_defect += (SCALE - contrast[v]).max(0);
        }
        QuantumReadout {
            probs,
            contrast,
            born_l1,
            contrast_defect,
            norm_err,
        }
    }
}

/// Unitary 2x2 SO(2) Givens spiral-arc interferometer and chiral probability current readout.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseInterferometer;

impl PhaseInterferometer {
    pub const MAX_THETA: i64 = HALF_PI >> 1;

    pub fn apply(&self, walk: &mut CoinedWebWalk, theta_mix: &[i64; STRANDS]) -> InterferometerReadout {
        let mut chiral = [0i64; STRANDS];
        let mut chiral_total = 0i64;

        for v in 0..STRANDS {
            let theta = clamp(theta_mix[v], -Self::MAX_THETA, Self::MAX_THETA);
            let (s, c) = sin_cos(theta);
            let a0 = walk.amp[v][0];
            let a1 = walk.amp[v][1];

            let new_a0 = ComplexQ30::new(
                mul_round(c, a0.re) - mul_round(s, a1.re),
                mul_round(c, a0.im) - mul_round(s, a1.im),
            );
            let new_a1 = ComplexQ30::new(
                mul_round(s, a0.re) + mul_round(c, a1.re),
                mul_round(s, a0.im) + mul_round(c, a1.im),
            );
            walk.amp[v][0] = new_a0;
            walk.amp[v][1] = new_a1;

            let mag_asym = (new_a0.norm_sq() - new_a1.norm_sq()).abs();
            let imag_cur = (mul_round(new_a0.re, new_a1.im) - mul_round(new_a0.im, new_a1.re)).abs();
            let cv = mag_asym + imag_cur;
            chiral[v] = cv;
            chiral_total += cv;
        }

        let (probs, norm_err) = walk.probs();
        let contrast = walk.contrast();
        let mut born_l1 = 0i64;
        let mut contrast_defect = 0i64;
        for v in 0..STRANDS {
            born_l1 += (probs[v] - UNIFORM_PROB).abs();
            contrast_defect += (SCALE - contrast[v]).max(0);
        }

        InterferometerReadout {
            probs,
            contrast,
            chiral,
            chiral_total,
            born_l1,
            contrast_defect,
            norm_err,
        }
    }
}

/// Phase-blind classical Markov random-walk replacement on the identical 64-arc graph.
#[derive(Clone, Debug)]
pub struct ClassicalWalkBase {
    pub prob: [[i64; ARCS]; STRANDS],
}

impl Default for ClassicalWalkBase {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassicalWalkBase {
    pub const fn new() -> Self {
        Self {
            prob: [[SCALE >> 6; ARCS]; STRANDS],
        }
    }

    pub fn step(&mut self, activity: &[i64; STRANDS], connected: bool) -> QuantumReadout {
        let mut next = [[0i64; ARCS]; STRANDS];
        for v in 0..STRANDS {
            let total_v = self.prob[v][0] + self.prob[v][1] + self.prob[v][2] + self.prob[v][3];
            let bias = clamp(activity[v].abs() >> 2, 0, QUARTER);
            let stay = QUARTER + bias;
            let p3 = mul_round(total_v, stay).clamp(0, total_v);
            let rem = total_v - p3;
            let p0 = rem / 3;
            let p1 = rem / 3;
            let p2 = rem - p0 - p1;

            next[cw(v, connected)][1] = p0;
            next[ccw(v, connected)][0] = p1;
            next[chord(v, connected)][2] = p2;
            next[v][3] = p3;
        }
        self.prob = next;

        let mut probs = [0i64; STRANDS];
        let mut total = 0i64;
        let mut born_l1 = 0i64;
        for v in 0..STRANDS {
            let p = self.prob[v][0] + self.prob[v][1] + self.prob[v][2] + self.prob[v][3];
            probs[v] = p;
            total += p;
            born_l1 += (p - UNIFORM_PROB).abs();
        }
        let _ = HALF;
        QuantumReadout {
            probs,
            contrast: [QUARTER; STRANDS],
            born_l1,
            contrast_defect: 0,
            norm_err: (total - SCALE).abs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unitary_norm_conserved_without_renormalization() {
        let mut walk = CoinedWebWalk::new();
        let mut ps = [0i64; STRANDS];
        let mut pc = [0i64; STRANDS];
        let mut pa = [0i64; STRANDS];
        for v in 0..STRANDS {
            ps[v] = ((v as i64) * 3 - 20) * (SCALE / 40);
            pc[v] = ((v as i64) + 1) * (SCALE / 50);
            pa[v] = (SCALE / 30) * (((v as i64) & 3) + 1);
        }
        let mut max_err = 0i64;
        for _ in 0..400 {
            let r = walk.step(&ps, &pc, &pa, true);
            if r.norm_err > max_err {
                max_err = r.norm_err;
            }
        }
        assert!(max_err <= SCALE / 10_000, "norm err drifted: {max_err}");
    }

    #[test]
    fn phase_interferometer_preserves_norm_and_distinguishes_phase() {
        let mut q1 = CoinedWebWalk::new();
        let mut q2 = CoinedWebWalk::new();
        let mut c1 = ClassicalWalkBase::new();
        let mut c2 = ClassicalWalkBase::new();

        let ps_pos = [HALF; STRANDS];
        let ps_neg = [-HALF; STRANDS];
        let zero = [0i64; STRANDS];
        let act = [QUARTER; STRANDS];
        let mut theta = [0i64; STRANDS];
        for v in 0..STRANDS {
            theta[v] = ((v as i64) - 8) * (SCALE / 32);
        }
        let pi = PhaseInterferometer;

        let mut ch1 = 0i64;
        let mut max_err = 0i64;
        for _ in 0..400 {
            q1.step(&ps_pos, &zero, &zero, true);
            q2.step(&ps_neg, &zero, &zero, true);
            let ir1 = pi.apply(&mut q1, &theta);
            let ir2 = pi.apply(&mut q2, &theta);
            ch1 = ir1.chiral_total;
            max_err = max_err.max(ir1.norm_err).max(ir2.norm_err);
            c1.step(&act, true);
            c2.step(&act, true);
        }
        assert!(max_err <= SCALE / 10_000, "unitary interferometer norm drifted: {max_err}");
        assert!(ch1 > 0, "expected positive chiral current");
        assert_eq!(c1.step(&act, true).probs, c2.step(&act, true).probs);
        let ir1 = pi.apply(&mut q1, &theta);
        let ir2 = pi.apply(&mut q2, &theta);
        assert_ne!(ir1.chiral_total, ir2.chiral_total);
    }
}
