//! Unsupervised information-theoretic channel estimators:
//! pointwise self-information, spatial Shannon entropy, and KL divergence.

use crate::fixed::{clamp, div, ln2, mul, SCALE};
use crate::topology::{BINS, STRANDS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoReadout {
    pub surprise: [i64; STRANDS],
    pub kl_div: i64,
    pub spatial_entropy: i64,
    pub surprise_mean: i64,
}

#[derive(Clone, Debug)]
pub struct InfoChannel {
    pub bin_counts: [[i64; BINS]; STRANDS],
    pub bg_spatial: [i64; STRANDS],
    pub bg_disp: [i64; STRANDS],
    pub bg_surp: [i64; STRANDS],
    pub surprise_state: [i64; STRANDS],
}

impl Default for InfoChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl InfoChannel {
    pub const fn new() -> Self {
        Self {
            bin_counts: [[SCALE >> 3; BINS]; STRANDS],
            bg_spatial: [SCALE >> 4; STRANDS],
            bg_disp: [SCALE >> 3; STRANDS],
            bg_surp: [SCALE; STRANDS],
            surprise_state: [SCALE; STRANDS],
        }
    }

    pub fn step(
        &mut self,
        active_mask: u16,
        bins: &[u8; STRANDS],
        disp: &[i64; STRANDS],
        spatial_raw: &[i64; STRANDS],
    ) -> InfoReadout {
        // Normalize spatial distribution strictly positive with tiny Laplace floor.
        let floor = SCALE >> 12;
        let mut p = [0i64; STRANDS];
        let mut raw_sum = 0i64;
        for v in 0..STRANDS {
            p[v] = spatial_raw[v].max(0) + floor;
            raw_sum += p[v];
        }
        let mut norm_sum = 0i64;
        for v in 0..STRANDS {
            p[v] = div(p[v], raw_sum).max(1);
            norm_sum += p[v];
        }
        p[0] += SCALE - norm_sum;

        let mut spatial_entropy = 0i64;
        let mut kl_div = 0i64;
        for v in 0..STRANDS {
            let log_p = ln2(p[v]);
            let log_q = ln2(self.bg_spatial[v]);
            spatial_entropy -= mul(p[v], log_p);
            kl_div += mul(p[v], log_p - log_q);
        }
        spatial_entropy = spatial_entropy.max(0);
        kl_div = kl_div.max(0);

        let mut bg_sum = 0i64;
        for v in 0..STRANDS {
            self.bg_spatial[v] = (((self.bg_spatial[v] as i128 * 31 + p[v] as i128) >> 5) as i64).max(1);
            bg_sum += self.bg_spatial[v];
        }
        self.bg_spatial[0] += SCALE - bg_sum;

        let mut surprise_sum = 0i64;
        for v in 0..STRANDS {
            let active = (active_mask & (1u16 << v)) != 0;
            let disp_abs = clamp(disp[v].abs(), 0, 16 * SCALE);
            let disp_dev = (disp_abs - self.bg_disp[v]).abs();
            if active {
                let b = (bins[v] as usize) & (BINS - 1);
                let q_b = self.bin_counts[v][b].max(1);
                let self_info = (-ln2(q_b)).max(0);
                let inst = self_info + (disp_dev << 1);
                self.surprise_state[v] = ((self.surprise_state[v] * 7) + inst) >> 3;
                self.bg_surp[v] = ((self.bg_surp[v] * 63) + self.surprise_state[v]) >> 6;

                let mut bsum = 0i64;
                for k in 0..BINS {
                    self.bin_counts[v][k] = ((self.bin_counts[v][k] * 15) >> 4).max(1);
                    bsum += self.bin_counts[v][k];
                }
                self.bin_counts[v][b] += SCALE - bsum;
                self.bg_disp[v] = ((self.bg_disp[v] * 31 + disp_abs) >> 5).max(0);
            } else {
                // Rate-invariant relaxation toward rolling conditional surprise baseline.
                self.surprise_state[v] = ((self.surprise_state[v] * 63) + self.bg_surp[v]) >> 6;
            }
            surprise_sum += self.surprise_state[v];
        }

        InfoReadout {
            surprise: self.surprise_state,
            kl_div,
            spatial_entropy,
            surprise_mean: surprise_sum >> 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_normalization_and_novelty_spike() {
        let mut info = InfoChannel::new();
        let bins_habit = [0u8; STRANDS];
        let zero = [0i64; STRANDS];
        let uni = [SCALE >> 4; STRANDS];
        for _ in 0..96 {
            let r = info.step(0xFFFF, &bins_habit, &zero, &uni);
            assert!(r.kl_div >= 0);
            for v in 0..STRANDS {
                let sum: i64 = info.bin_counts[v].iter().sum();
                assert_eq!(sum, SCALE);
            }
        }
        let steady = info.step(0xFFFF, &bins_habit, &zero, &uni).surprise[0];
        let bins_novel = [5u8; STRANDS];
        let spike = info.step(0xFFFF, &bins_novel, &zero, &uni).surprise[0];
        assert!(spike > steady + SCALE / 4, "steady={steady}, spike={spike}");
    }

    #[test]
    fn rate_invariant_habitual_vs_uniform() {
        let mut ch_habit = InfoChannel::new();
        let mut ch_sparse_uni = InfoChannel::new();
        let zero = [0i64; STRANDS];
        let uni = [SCALE >> 4; STRANDS];
        let mut last_habit = 0i64;
        let mut last_uni = 0i64;
        for t in 0..160 {
            let mut b_h = [0u8; STRANDS];
            let mut b_u = [0u8; STRANDS];
            for v in 0..STRANDS {
                b_h[v] = ((v + (t >> 2)) & 1) as u8;
                b_u[v] = ((t * 5 + v * 3) % 8) as u8;
            }
            last_habit = ch_habit.step(0xFFFF, &b_h, &zero, &uni).surprise_mean;
            let active_half = if t % 2 == 0 { 0xFFFF } else { 0 };
            last_uni = ch_sparse_uni.step(active_half, &b_u, &zero, &uni).surprise_mean;
        }
        assert!(last_uni > last_habit + SCALE / 2, "habit={last_habit}, sparse_uni={last_uni}");
    }
}
