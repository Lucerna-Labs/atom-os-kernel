extern crate atom_web_engine;
extern crate atom_web_fp;

use atom_web_engine::{BaseMode, ChaosPos, StackConfig, StackOrder, WebEngine, NUM_SCALARS};
use atom_web_fp::{BusImpulse, HarmonicWeb, SCALE, STRANDS};
use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

const WARMUP_TICKS: usize = 128;
const CALIB_START: usize = 64;
const TEST_TICKS: usize = 256;
const TOTAL_TICKS: usize = WARMUP_TICKS + TEST_TICKS;
const SEEDS: [u64; 3] = [0, 1, 2];
const H2_LIMIT: i64 = SCALE / 10_000;
const H3_LIMIT: i64 = SCALE / 10_000;
const ABS_LIMIT: i64 = 1_000_000 * SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Regime {
    R0Quiet = 0,
    R1BenignBurst = 1,
    R2Flood = 2,
    R3PhaseJitter = 3,
    R4EntropyShift = 4,
    R5BackpressureStall = 5,
    R6Recovery = 6,
}

impl Regime {
    const ALL: [Regime; 7] = [
        Regime::R0Quiet,
        Regime::R1BenignBurst,
        Regime::R2Flood,
        Regime::R3PhaseJitter,
        Regime::R4EntropyShift,
        Regime::R5BackpressureStall,
        Regime::R6Recovery,
    ];
}

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn from_seed(regime: Regime, seed: u64) -> Self {
        let mut x = seed
            .wrapping_add(1)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ ((regime as u64).wrapping_add(1).wrapping_mul(0xbf58_476d_1ce4_e5b9));
        if x == 0 {
            x = 0x94d0_49bb_1331_11eb;
        }
        Self(x)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn range_q30(&mut self, lo_milli: i64, hi_milli: i64) -> i64 {
        let span = (hi_milli - lo_milli).max(1) as u64;
        let v = lo_milli + (self.next_u64() % span) as i64;
        (SCALE * v) / 1000
    }
}

fn generate_tick_impulses(regime: Regime, tick: usize, rng: &mut Rng) -> [BusImpulse; STRANDS] {
    let mut out = [BusImpulse::default(); STRANDS];
    let effective_regime = if tick < WARMUP_TICKS {
        Regime::R0Quiet
    } else if regime == Regime::R6Recovery {
        if tick < WARMUP_TICKS + 96 {
            Regime::R6Recovery
        } else {
            Regime::R0Quiet
        }
    } else {
        regime
    };

    // Baseline R0_quiet sparse activity across all 16 strands.
    for v in 0..STRANDS {
        out[v].strand = v as u8;
        if (rng.next_u64() & 7) == 0 {
            let sign = if ((tick + v) & 1) == 0 { 1 } else { -1 };
            out[v] = BusImpulse {
                strand: v as u8,
                bin: ((v + (tick >> 3)) & 1) as u8,
                magnitude: rng.range_q30(80, 150),
                dispersion: rng.range_q30(40, 60),
                phase_skew: sign * rng.range_q30(35, 65),
                queue_pressure: rng.range_q30(15, 25),
                active: true,
            };
        }
    }

    match effective_regime {
        Regime::R0Quiet => {}
        Regime::R1BenignBurst => {
            if (tick - WARMUP_TICKS) % 64 < 32 {
                for v in 0..=4 {
                    let sign = if ((tick + v) & 1) == 0 { 1 } else { -1 };
                    out[v] = BusImpulse {
                        strand: v as u8,
                        bin: ((v + (tick >> 4)) % 3) as u8,
                        magnitude: rng.range_q30(450, 700),
                        dispersion: rng.range_q30(100, 140),
                        phase_skew: sign * rng.range_q30(50, 100),
                        queue_pressure: rng.range_q30(120, 180),
                        active: true,
                    };
                }
            }
        }
        Regime::R2Flood => {
            let sign = if (tick & 1) == 0 { 1 } else { -1 };
            out[7] = BusImpulse {
                strand: 7,
                bin: (rng.next_u64() % 8) as u8,
                magnitude: rng.range_q30(1350, 1650),
                dispersion: rng.range_q30(400, 500),
                phase_skew: sign * rng.range_q30(180, 320),
                queue_pressure: rng.range_q30(850, 1100),
                active: true,
            };
        }
        Regime::R3PhaseJitter => {
            for &v in &[2usize, 5, 11, 14] {
                if ((tick + v) & 1) == 0 {
                    let sign = if ((tick >> 1) + v) & 1 == 0 { 1 } else { -1 };
                    out[v] = BusImpulse {
                        strand: v as u8,
                        bin: ((v + (tick >> 3)) & 1) as u8,
                        magnitude: rng.range_q30(120, 220),
                        dispersion: rng.range_q30(50, 70),
                        phase_skew: sign * rng.range_q30(1100, 1500),
                        queue_pressure: rng.range_q30(30, 50),
                        active: true,
                    };
                }
            }
        }
        Regime::R4EntropyShift => {
            for &v in &[1usize, 6, 10, 13] {
                if ((tick + v) & 1) == 0 {
                    let sign = if ((tick + v) & 1) == 0 { 1 } else { -1 };
                    out[v] = BusImpulse {
                        strand: v as u8,
                        bin: (rng.next_u64() % 8) as u8,
                        magnitude: rng.range_q30(120, 180),
                        dispersion: rng.range_q30(80, 120),
                        phase_skew: sign * rng.range_q30(40, 80),
                        queue_pressure: rng.range_q30(20, 40),
                        active: true,
                    };
                }
            }
        }
        Regime::R5BackpressureStall => {
            for &v in &[9usize, 10] {
                out[v] = BusImpulse {
                    strand: v as u8,
                    bin: (v & 1) as u8,
                    magnitude: rng.range_q30(250, 350),
                    dispersion: rng.range_q30(450, 550),
                    phase_skew: rng.range_q30(400, 500),
                    queue_pressure: rng.range_q30(700, 950),
                    active: true,
                };
            }
        }
        Regime::R6Recovery => {
            for &v in &[3usize, 7, 12] {
                let sign = if ((tick + v) & 1) == 0 { 1 } else { -1 };
                out[v] = BusImpulse {
                    strand: v as u8,
                    bin: (rng.next_u64() % 8) as u8,
                    magnitude: rng.range_q30(900, 1100),
                    dispersion: rng.range_q30(300, 400),
                    phase_skew: sign * rng.range_q30(800, 1000),
                    queue_pressure: rng.range_q30(650, 850),
                    active: true,
                };
            }
        }
    }

    out
}

/// Clearly labeled conventional non-Atom baseline (per-strand EWMA tracker).
struct EwmaBaseline {
    ewma_mag: [f64; STRANDS],
    ewma_disp: [f64; STRANDS],
    ewma_press: [f64; STRANDS],
    bin_probs: [[f64; 8]; STRANDS],
}

impl EwmaBaseline {
    fn new() -> Self {
        Self {
            ewma_mag: [0.0; STRANDS],
            ewma_disp: [0.0; STRANDS],
            ewma_press: [0.0; STRANDS],
            bin_probs: [[0.125; 8]; STRANDS],
        }
    }

    fn step(&mut self, impulses: &[BusImpulse; STRANDS]) -> ([f64; 4], [f64; STRANDS]) {
        let mut mag_sum = 0.0;
        let mut disp_sum = 0.0;
        let mut press_sum = 0.0;
        let mut surp_sum = 0.0;
        let mut strand_resp = [0.0f64; STRANDS];
        let s = SCALE as f64;

        for v in 0..STRANDS {
            let imp = &impulses[v];
            let (m, d, p, surp) = if imp.active {
                let m = (imp.magnitude as f64) / s;
                let d = (imp.dispersion as f64) / s;
                let p = (imp.queue_pressure as f64) / s;
                let b = (imp.bin as usize) & 7;
                let q = self.bin_probs[v][b].max(1e-6);
                let surp = -q.log2();
                for k in 0..8 {
                    self.bin_probs[v][k] *= 15.0 / 16.0;
                }
                self.bin_probs[v][b] += 1.0 / 16.0;
                (m, d, p, surp)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };

            self.ewma_mag[v] = 0.75 * self.ewma_mag[v] + 0.25 * m;
            self.ewma_disp[v] = 0.75 * self.ewma_disp[v] + 0.25 * d;
            self.ewma_press[v] = 0.75 * self.ewma_press[v] + 0.25 * p;

            mag_sum += self.ewma_mag[v];
            disp_sum += self.ewma_disp[v];
            press_sum += self.ewma_press[v];
            surp_sum += surp;
            strand_resp[v] = self.ewma_mag[v] + self.ewma_disp[v] + self.ewma_press[v] + surp * 0.25;
        }

        ([mag_sum, disp_sum, press_sum, surp_sum / 16.0], strand_resp)
    }
}

#[derive(Clone, Copy)]
enum ArmKind {
    Atom(StackConfig),
    NonAtomEwma,
}

#[derive(Clone, Copy)]
struct ArmSpec {
    name: &'static str,
    group: &'static str,
    kind: ArmKind,
}

fn build_arms() -> Vec<ArmSpec> {
    let mut arms = Vec::with_capacity(41);
    let chaos_variants: [(&'static str, ChaosPos, ChaosPos); 10] = [
        ("NoChaos", ChaosPos::Off, ChaosPos::Off),
        ("X1_Early", ChaosPos::Early, ChaosPos::Off),
        ("X1_Mid", ChaosPos::Mid, ChaosPos::Off),
        ("X1_Late", ChaosPos::Late, ChaosPos::Off),
        ("X2_Early", ChaosPos::Off, ChaosPos::Early),
        ("X2_Mid", ChaosPos::Off, ChaosPos::Mid),
        ("X2_Late", ChaosPos::Off, ChaosPos::Late),
        ("X1Early_X2Mid", ChaosPos::Early, ChaosPos::Mid),
        ("X1Mid_X2Late", ChaosPos::Mid, ChaosPos::Late),
        ("X1Early_X2Late", ChaosPos::Early, ChaosPos::Late),
    ];

    for (order_name, order) in [
        ("OrderA", StackOrder::OrderA),
        ("OrderB", StackOrder::OrderB),
        ("OrderC", StackOrder::OrderC),
    ] {
        for (cname, x1, x2) in chaos_variants {
            let full_name: &'static str = Box::leak(format!("{order_name}_{cname}").into_boxed_str());
            arms.push(ArmSpec {
                name: full_name,
                group: order_name,
                kind: ArmKind::Atom(StackConfig::full(order).with_chaos(x1, x2)),
            });
        }
    }

    // Rule 4 quantum controls
    arms.push(ArmSpec {
        name: "Ctrl_BaseOnly",
        group: "Ctrl_Quantum",
        kind: ArmKind::Atom(StackConfig {
            order: StackOrder::OrderA,
            base: BaseMode::QuantumFull,
            mechanics: false,
            physics: false,
            info: false,
            chemistry: false,
            cognition: false,
            connected: true,
            x1: ChaosPos::Off,
            x2: ChaosPos::Off,
        }),
    });
    arms.push(ArmSpec {
        name: "Ctrl_ClassicalUpperOnly",
        group: "Ctrl_Quantum",
        kind: ArmKind::Atom(StackConfig {
            base: BaseMode::NoBase,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Ctrl_ReplacementBaseDiffusion",
        group: "Ctrl_Quantum",
        kind: ArmKind::Atom(StackConfig {
            base: BaseMode::ClassicalWalkReplacement,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });

    // Rule 6 & Topology ablations
    arms.push(ArmSpec {
        name: "Abl_NoPhaseInterferometer",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            base: BaseMode::QuantumNoInterferometer,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Abl_NoMechanics",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            mechanics: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Abl_NoPhysics",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            physics: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Abl_NoInfo",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            info: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Abl_NoChemistry",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            chemistry: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Abl_NoCognition",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            cognition: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });
    arms.push(ArmSpec {
        name: "Ctrl_DisconnectedWeb",
        group: "Ablation",
        kind: ArmKind::Atom(StackConfig {
            connected: false,
            ..StackConfig::full(StackOrder::OrderA)
        }),
    });

    // Non-Atom baseline
    arms.push(ArmSpec {
        name: "Baseline_NonAtom_Ewma",
        group: "Baseline",
        kind: ArmKind::NonAtomEwma,
    });

    assert_eq!(arms.len(), 41);
    arms
}

fn mann_whitney_auroc(pos: &[f64], neg: &[f64]) -> f64 {
    if pos.is_empty() || neg.is_empty() {
        return 0.5;
    }
    let mut items: Vec<(f64, bool)> = Vec::with_capacity(pos.len() + neg.len());
    for &p in pos {
        items.push((p, true));
    }
    for &n in neg {
        items.push((n, false));
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut rank_sum_pos = 0.0f64;
    let mut i = 0usize;
    while i < items.len() {
        let mut j = i + 1;
        while j < items.len() && (items[j].0 - items[i].0).abs() <= 1e-12 {
            j += 1;
        }
        let avg_rank = 0.5 * ((i + 1 + j) as f64);
        for item in &items[i..j] {
            if item.1 {
                rank_sum_pos += avg_rank;
            }
        }
        i = j;
    }
    let np = pos.len() as f64;
    let nn = neg.len() as f64;
    (rank_sum_pos - np * (np + 1.0) * 0.5) / (np * nn)
}

fn check_undriven_mech_decay_h5() -> bool {
    let mut web = HarmonicWeb::new();
    let mut kick = [0i64; STRANDS];
    kick[4] = 2 * SCALE;
    let zero = [0i64; STRANDS];
    for _ in 0..4 {
        web.step(&kick, &zero, true);
    }
    let mut prev_e = web.step(&zero, &zero, true).total_energy;
    for _ in 0..120 {
        let r = web.step(&zero, &zero, true);
        if r.total_energy > prev_e {
            return false;
        }
        prev_e = r.total_energy;
    }
    true
}

struct ArmResult {
    name: &'static str,
    group: &'static str,
    j: f64,
    auroc_all: f64,
    auroc_hard: f64,
    web_prop: f64,
    recovery: f64,
    recovery_ratio: f64,
    ns_per_tick: f64,
    max_norm_err_q30: i64,
    max_chem_err_q30: i64,
    max_antisym_err_q30: i64,
    min_temp_q30: i64,
    min_sigma_q30: i64,
    min_species_q30: i64,
    x1_mean_spread: f64,
    x2_mean_spread: f64,
    h1_det: bool,
    h2_unit: bool,
    h3_mass: bool,
    h4_thermo: bool,
    h5_decay: bool,
    h6_lat: bool,
    h7_nontriv: bool,
    all_h_pass: bool,
    auroc_r2: f64,
    auroc_r3: f64,
    auroc_r4: f64,
    auroc_r5: f64,
}

fn evaluate_arm(arm: &ArmSpec, mech_decay_ok: bool) -> ArmResult {
    let s_f = SCALE as f64;
    let mut all_pos: Vec<f64> = Vec::with_capacity(4 * 3 * TEST_TICKS);
    let mut all_neg: Vec<f64> = Vec::with_capacity(2 * 3 * TEST_TICKS);
    let mut hard_pos: Vec<f64> = Vec::with_capacity(3 * 3 * TEST_TICKS);
    let mut burst_neg: Vec<f64> = Vec::with_capacity(3 * TEST_TICKS);
    let mut r2_pos = Vec::with_capacity(3 * TEST_TICKS);
    let mut r3_pos = Vec::with_capacity(3 * TEST_TICKS);
    let mut r4_pos = Vec::with_capacity(3 * TEST_TICKS);
    let mut r5_pos = Vec::with_capacity(3 * TEST_TICKS);

    let mut quiet_remote_r2 = Vec::with_capacity(3 * TEST_TICKS);
    let mut pert_remote_r2 = Vec::with_capacity(3 * TEST_TICKS);
    let mut quiet_remote_r5 = Vec::with_capacity(3 * TEST_TICKS);
    let mut pert_remote_r5 = Vec::with_capacity(3 * TEST_TICKS);
    let mut r6_ratios = Vec::with_capacity(3);

    let mut max_norm_err = 0i64;
    let mut max_chem_err = 0i64;
    let mut max_antisym_err = 0i64;
    let mut min_temp = i64::MAX;
    let mut min_sigma = i64::MAX;
    let mut min_species = i64::MAX;
    let mut bounded_ok = true;
    let mut h1_det = true;

    let mut active_mask = 0u16;
    let mut scalar_vals_all: Vec<Vec<f64>> = vec![Vec::new(); NUM_SCALARS];
    let mut x1_spread_sum = 0.0f64;
    let mut x2_spread_sum = 0.0f64;
    let mut total_test_ticks = 0usize;

    // Duplicate-run determinism verification on R3 seed 0.
    if let ArmKind::Atom(cfg) = arm.kind {
        let mut e1 = WebEngine::new(cfg);
        let mut e2 = WebEngine::new(cfg);
        let mut rng1 = Rng::from_seed(Regime::R3PhaseJitter, 0);
        let mut rng2 = Rng::from_seed(Regime::R3PhaseJitter, 0);
        for t in 0..160 {
            let i1 = generate_tick_impulses(Regime::R3PhaseJitter, t, &mut rng1);
            let i2 = generate_tick_impulses(Regime::R3PhaseJitter, t, &mut rng2);
            if e1.step(&i1) != e2.step(&i2) {
                h1_det = false;
                break;
            }
        }
    }

    for &regime in &Regime::ALL {
        for &seed in &SEEDS {
            let mut rng = Rng::from_seed(regime, seed);
            let mut atom_eng = match arm.kind {
                ArmKind::Atom(cfg) => Some(WebEngine::new(cfg)),
                ArmKind::NonAtomEwma => None,
            };
            let mut ewma_eng = match arm.kind {
                ArmKind::NonAtomEwma => Some(EwmaBaseline::new()),
                ArmKind::Atom(_) => None,
            };

            let dim_count = match arm.kind {
                ArmKind::Atom(cfg) => {
                    active_mask = cfg.active_obs_mask();
                    12
                }
                ArmKind::NonAtomEwma => {
                    active_mask = 0x0F;
                    4
                }
            };

            let mut scalars_trace: Vec<[f64; 12]> = Vec::with_capacity(TOTAL_TICKS);
            let mut strand_trace: Vec<[f64; STRANDS]> = Vec::with_capacity(TOTAL_TICKS);

            for t in 0..TOTAL_TICKS {
                let imps = generate_tick_impulses(regime, t, &mut rng);
                let mut sc = [0.0f64; 12];
                let mut sr = [0.0f64; STRANDS];

                if let Some(eng) = &mut atom_eng {
                    let obs = eng.step(&imps);
                    for k in 0..NUM_SCALARS {
                        if obs.scalars[k].abs() >= ABS_LIMIT {
                            bounded_ok = false;
                        }
                        sc[k] = (obs.scalars[k] as f64) / s_f;
                        if t >= WARMUP_TICKS {
                            scalar_vals_all[k].push(sc[k]);
                        }
                    }
                    for v in 0..STRANDS {
                        if obs.strand_response[v].abs() >= ABS_LIMIT {
                            bounded_ok = false;
                        }
                        sr[v] = (obs.strand_response[v] as f64) / s_f;
                    }
                    max_norm_err = max_norm_err.max(obs.unitary_norm_err);
                    max_chem_err = max_chem_err.max(obs.chem_mass_err);
                    max_antisym_err = max_antisym_err.max(obs.antisym_err);
                    min_temp = min_temp.min(obs.min_temp);
                    min_sigma = min_sigma.min(obs.min_entropy_prod);
                    min_species = min_species.min(obs.min_species);
                    if t >= WARMUP_TICKS {
                        x1_spread_sum += (obs.x1_spread as f64) / s_f;
                        x2_spread_sum += (obs.x2_spread as f64) / s_f;
                        total_test_ticks += 1;
                    }
                } else if let Some(ewma) = &mut ewma_eng {
                    let (v4, resp) = ewma.step(&imps);
                    for k in 0..4 {
                        sc[k] = v4[k];
                        if t >= WARMUP_TICKS {
                            scalar_vals_all[k].push(sc[k]);
                        }
                    }
                    sr = resp;
                }
                scalars_trace.push(sc);
                strand_trace.push(sr);
            }

            // Warmup calibration strictly from ticks 64..128.
            let mut mu = [0.0f64; 12];
            let mut sd = [0.0f64; 12];
            for k in 0..dim_count {
                if (active_mask & (1u16 << k)) == 0 {
                    continue;
                }
                let mut sum = 0.0;
                for t in CALIB_START..WARMUP_TICKS {
                    sum += scalars_trace[t][k];
                }
                let mean = sum / ((WARMUP_TICKS - CALIB_START) as f64);
                let mut var = 0.0;
                for t in CALIB_START..WARMUP_TICKS {
                    let diff = scalars_trace[t][k] - mean;
                    var += diff * diff;
                }
                mu[k] = mean;
                sd[k] = (var / ((WARMUP_TICKS - CALIB_START) as f64)).sqrt().max(1e-4);
            }

            let mut s_mu = [0.0f64; STRANDS];
            let mut s_sd = [0.0f64; STRANDS];
            for v in 0..STRANDS {
                let mut sum = 0.0;
                for t in CALIB_START..WARMUP_TICKS {
                    sum += strand_trace[t][v];
                }
                let mean = sum / ((WARMUP_TICKS - CALIB_START) as f64);
                let mut var = 0.0;
                for t in CALIB_START..WARMUP_TICKS {
                    let diff = strand_trace[t][v] - mean;
                    var += diff * diff;
                }
                s_mu[v] = mean;
                s_sd[v] = (var / ((WARMUP_TICKS - CALIB_START) as f64)).sqrt().max(1e-4);
            }

            let n_active = active_mask.count_ones().max(1) as f64;
            let mut d_window = Vec::with_capacity(TEST_TICKS);
            for t in WARMUP_TICKS..TOTAL_TICKS {
                let mut sq = 0.0;
                for k in 0..dim_count {
                    if (active_mask & (1u16 << k)) != 0 {
                        let z = (scalars_trace[t][k] - mu[k]) / sd[k];
                        sq += z * z;
                    }
                }
                let d = (sq / n_active).sqrt();
                d_window.push(d);

                if regime == Regime::R0Quiet || regime == Regime::R2Flood {
                    let mut rem = 0.0;
                    for v in 0..STRANDS {
                        if v != 7 {
                            rem += (strand_trace[t][v] - s_mu[v]).abs() / s_sd[v];
                        }
                    }
                    rem /= 15.0;
                    if regime == Regime::R0Quiet {
                        quiet_remote_r2.push(rem);
                    } else {
                        pert_remote_r2.push(rem);
                    }
                }
                if regime == Regime::R0Quiet || regime == Regime::R5BackpressureStall {
                    let mut rem = 0.0;
                    for v in 0..STRANDS {
                        if v != 9 && v != 10 {
                            rem += (strand_trace[t][v] - s_mu[v]).abs() / s_sd[v];
                        }
                    }
                    rem /= 14.0;
                    if regime == Regime::R0Quiet {
                        quiet_remote_r5.push(rem);
                    } else {
                        pert_remote_r5.push(rem);
                    }
                }
            }

            match regime {
                Regime::R0Quiet => all_neg.extend_from_slice(&d_window),
                Regime::R1BenignBurst => {
                    all_neg.extend_from_slice(&d_window);
                    burst_neg.extend_from_slice(&d_window);
                }
                Regime::R2Flood => {
                    all_pos.extend_from_slice(&d_window);
                    r2_pos.extend_from_slice(&d_window);
                }
                Regime::R3PhaseJitter => {
                    all_pos.extend_from_slice(&d_window);
                    hard_pos.extend_from_slice(&d_window);
                    r3_pos.extend_from_slice(&d_window);
                }
                Regime::R4EntropyShift => {
                    all_pos.extend_from_slice(&d_window);
                    hard_pos.extend_from_slice(&d_window);
                    r4_pos.extend_from_slice(&d_window);
                }
                Regime::R5BackpressureStall => {
                    all_pos.extend_from_slice(&d_window);
                    hard_pos.extend_from_slice(&d_window);
                    r5_pos.extend_from_slice(&d_window);
                }
                Regime::R6Recovery => {
                    let peak = d_window[..128]
                        .iter()
                        .copied()
                        .fold(0.0f64, |a, b| if b > a { b } else { a });
                    let final_mean = d_window[224..256].iter().sum::<f64>() / 32.0;
                    r6_ratios.push(final_mean / (peak + 1e-9));
                }
            }
        }
    }

    // Metrics
    let auroc_all = mann_whitney_auroc(&all_pos, &all_neg);
    let auroc_hard = mann_whitney_auroc(&hard_pos, &burst_neg);
    let auroc_r2 = mann_whitney_auroc(&r2_pos, &burst_neg);
    let auroc_r3 = mann_whitney_auroc(&r3_pos, &burst_neg);
    let auroc_r4 = mann_whitney_auroc(&r4_pos, &burst_neg);
    let auroc_r5 = mann_whitney_auroc(&r5_pos, &burst_neg);

    let prop_pair = |quiet: &[f64], pert: &[f64]| -> f64 {
        let qm = quiet.iter().sum::<f64>() / quiet.len() as f64;
        let qstd = (quiet.iter().map(|x| (x - qm) * (x - qm)).sum::<f64>() / quiet.len() as f64).sqrt();
        let pm = pert.iter().sum::<f64>() / pert.len() as f64;
        let r = (pm - qm).max(0.0) / (qstd + 1e-4);
        r / (1.0 + r)
    };
    let web_prop = 0.5 * (prop_pair(&quiet_remote_r2, &pert_remote_r2) + prop_pair(&quiet_remote_r5, &pert_remote_r5));
    let recovery_ratio = r6_ratios.iter().sum::<f64>() / (r6_ratios.len() as f64);
    let recovery = 1.0 - recovery_ratio.min(1.0);
    let j = 0.50 * auroc_all + 0.25 * auroc_hard + 0.15 * web_prop + 0.10 * recovery;

    // Microbenchmark H6 (20_000 ticks in -O)
    let mut bench_rng = Rng::from_seed(Regime::R1BenignBurst, 0);
    let bench_inputs: Vec<[BusImpulse; STRANDS]> = (0..256)
        .map(|t| generate_tick_impulses(Regime::R1BenignBurst, t + WARMUP_TICKS, &mut bench_rng))
        .collect();
    let bench_iters = 20_000usize;
    let start = Instant::now();
    match arm.kind {
        ArmKind::Atom(cfg) => {
            let mut eng = WebEngine::new(cfg);
            for i in 0..bench_iters {
                black_box(eng.step(black_box(&bench_inputs[i & 255])));
            }
        }
        ArmKind::NonAtomEwma => {
            let mut ewma = EwmaBaseline::new();
            for i in 0..bench_iters {
                black_box(ewma.step(black_box(&bench_inputs[i & 255])));
            }
        }
    }
    let ns_per_tick = (start.elapsed().as_nanos() as f64) / (bench_iters as f64);

    // Non-triviality H7
    let mut h7_nontriv = true;
    for k in 0..NUM_SCALARS {
        if (active_mask & (1u16 << k)) != 0 {
            let vals = &scalar_vals_all[k];
            let mean = vals.iter().sum::<f64>() / (vals.len() as f64);
            let var = vals.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (vals.len() as f64);
            if var.sqrt() <= 1e-6 {
                h7_nontriv = false;
            }
        }
    }

    let (h2_unit, h3_mass, h4_thermo) = match arm.kind {
        ArmKind::Atom(cfg) => {
            let quantum_on = matches!(
                cfg.base,
                BaseMode::QuantumFull | BaseMode::QuantumNoInterferometer | BaseMode::ClassicalWalkReplacement
            );
            (
                !quantum_on || max_norm_err <= H2_LIMIT,
                !cfg.chemistry || (max_chem_err <= H3_LIMIT && min_species >= 0),
                !cfg.physics || (min_sigma >= 0 && min_temp >= atom_web_fp::T_MIN),
            )
        }
        ArmKind::NonAtomEwma => (true, true, true),
    };
    let h1 = h1_det && bounded_ok;
    let h5 = mech_decay_ok && recovery_ratio <= 0.35;
    let h6 = ns_per_tick <= 15_000.0;
    let all_h_pass = h1 && h2_unit && h3_mass && h4_thermo && h5 && h6 && h7_nontriv;

    let denom = total_test_ticks.max(1) as f64;
    ArmResult {
        name: arm.name,
        group: arm.group,
        j,
        auroc_all,
        auroc_hard,
        web_prop,
        recovery,
        recovery_ratio,
        ns_per_tick,
        max_norm_err_q30: max_norm_err,
        max_chem_err_q30: max_chem_err,
        max_antisym_err_q30: max_antisym_err,
        min_temp_q30: if min_temp == i64::MAX { SCALE } else { min_temp },
        min_sigma_q30: if min_sigma == i64::MAX { 0 } else { min_sigma },
        min_species_q30: if min_species == i64::MAX { 0 } else { min_species },
        x1_mean_spread: x1_spread_sum / denom,
        x2_mean_spread: x2_spread_sum / denom,
        h1_det: h1,
        h2_unit,
        h3_mass,
        h4_thermo,
        h5_decay: h5,
        h6_lat: h6,
        h7_nontriv,
        all_h_pass,
        auroc_r2,
        auroc_r3,
        auroc_r4,
        auroc_r5,
    }
}

fn write_outputs(results: &[ArmResult]) -> std::io::Result<()> {
    let out_dir = Path::new("/media/jesse/Disk2/Projects/atom-os-kernel/experiments/c1-spiderweb/results-v2");
    fs::create_dir_all(out_dir)?;

    let mut csv = fs::File::create(out_dir.join("summary.csv"))?;
    writeln!(
        csv,
        "name,group,J,auroc_all,auroc_hard,web_prop,recovery,recovery_ratio,auroc_r2,auroc_r3,auroc_r4,auroc_r5,ns_per_tick,x1_spread,x2_spread,h1,h2,h3,h4,h5,h6,h7,all_h_pass"
    )?;
    for r in results {
        writeln!(
            csv,
            "{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.1},{:.6},{:.6},{},{},{},{},{},{},{},{}",
            r.name, r.group, r.j, r.auroc_all, r.auroc_hard, r.web_prop, r.recovery, r.recovery_ratio,
            r.auroc_r2, r.auroc_r3, r.auroc_r4, r.auroc_r5, r.ns_per_tick,
            r.x1_mean_spread, r.x2_mean_spread,
            r.h1_det, r.h2_unit, r.h3_mass, r.h4_thermo, r.h5_decay, r.h6_lat, r.h7_nontriv, r.all_h_pass
        )?;
    }

    let mut json = fs::File::create(out_dir.join("report.json"))?;
    writeln!(json, "{{")?;
    writeln!(json, "  \"total_arms\": {},", results.len())?;
    writeln!(json, "  \"runs_per_arm\": 21,")?;
    writeln!(json, "  \"total_runs\": {},", results.len() * 21)?;
    writeln!(json, "  \"arms\": [")?;
    for (idx, r) in results.iter().enumerate() {
        let comma = if idx + 1 < results.len() { "," } else { "" };
        writeln!(
            json,
            "    {{\"name\":\"{}\",\"group\":\"{}\",\"J\":{:.6},\"auroc_all\":{:.6},\"auroc_hard\":{:.6},\"web_prop\":{:.6},\"recovery\":{:.6},\"recovery_ratio\":{:.6},\"auroc_r2\":{:.6},\"auroc_r3\":{:.6},\"auroc_r4\":{:.6},\"auroc_r5\":{:.6},\"ns_per_tick\":{:.1},\"max_norm_err_q30\":{},\"max_chem_err_q30\":{},\"max_antisym_err_q30\":{},\"min_temp_q30\":{},\"min_sigma_q30\":{},\"min_species_q30\":{},\"x1_spread\":{:.6},\"x2_spread\":{:.6},\"h\":[{},{},{},{},{},{},{}],\"all_h_pass\":{}}}{}",
            r.name, r.group, r.j, r.auroc_all, r.auroc_hard, r.web_prop, r.recovery, r.recovery_ratio,
            r.auroc_r2, r.auroc_r3, r.auroc_r4, r.auroc_r5, r.ns_per_tick,
            r.max_norm_err_q30, r.max_chem_err_q30, r.max_antisym_err_q30, r.min_temp_q30, r.min_sigma_q30, r.min_species_q30,
            r.x1_mean_spread, r.x2_mean_spread,
            r.h1_det, r.h2_unit, r.h3_mass, r.h4_thermo, r.h5_decay, r.h6_lat, r.h7_nontriv, r.all_h_pass, comma
        )?;
    }
    writeln!(json, "  ]")?;
    writeln!(json, "}}")?;
    Ok(())
}

fn find<'a>(results: &'a [ArmResult], name: &str) -> &'a ArmResult {
    results.iter().find(|r| r.name == name).unwrap()
}

fn main() {
    let mech_decay_ok = check_undriven_mech_decay_h5();
    let arms = build_arms();
    let results: Vec<ArmResult> = arms.iter().map(|a| evaluate_arm(a, mech_decay_ok)).collect();
    write_outputs(&results).expect("write CSV/JSON results");

    let mut ranked: Vec<&ArmResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.j.total_cmp(&a.j));

    println!("=== C1-V2 SPIDERWEB BUS ATOM EXPERIMENT (41 ARMS x 21 RUNS = 861 RUNS) ===");
    println!("Isolated HarmonicWeb undriven decay H5: {}", if mech_decay_ok { "PASS" } else { "FAIL" });
    println!(
        "{:<31} {:<12} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>8} {:>5}",
        "ARM", "GROUP", "J", "AUC_ALL", "AUC_HRD", "WEBPROP", "RECOV", "REC_RAT", "R2", "R3", "R4", "R5", "ns/tick", "H_OK"
    );
    for r in &ranked {
        let h_str = if r.all_h_pass {
            "PASS".to_string()
        } else {
            format!(
                "{}{}{}{}{}{}{}",
                if r.h1_det { '.' } else { '1' },
                if r.h2_unit { '.' } else { '2' },
                if r.h3_mass { '.' } else { '3' },
                if r.h4_thermo { '.' } else { '4' },
                if r.h5_decay { '.' } else { '5' },
                if r.h6_lat { '.' } else { '6' },
                if r.h7_nontriv { '.' } else { '7' }
            )
        };
        println!(
            "{:<31} {:<12} {:>7.4} {:>7.4} {:>7.4} {:>7.4} {:>7.4} {:>7.4} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>8.1} {:>5}",
            r.name, r.group, r.j, r.auroc_all, r.auroc_hard, r.web_prop, r.recovery, r.recovery_ratio,
            r.auroc_r2, r.auroc_r3, r.auroc_r4, r.auroc_r5, r.ns_per_tick, h_str
        );
    }

    println!("\n--- RULE 2 ORDER COMPARISON (NO CHAOS) ---");
    for n in ["OrderA_NoChaos", "OrderB_NoChaos", "OrderC_NoChaos"] {
        let r = find(&results, n);
        println!("{n:<20} J={:.4} AUC_all={:.4} AUC_hard={:.4} WebProp={:.4} Recov={:.4} H={}", r.j, r.auroc_all, r.auroc_hard, r.web_prop, r.recovery, r.all_h_pass);
    }

    println!("\n--- RULE 4 QUANTUM CONTROLS ---");
    for n in [
        "OrderA_NoChaos",
        "Ctrl_BaseOnly",
        "Ctrl_ClassicalUpperOnly",
        "Ctrl_ReplacementBaseDiffusion",
        "Abl_NoPhaseInterferometer",
    ] {
        let r = find(&results, n);
        println!("{n:<31} J={:.4} R2={:.3} R3={:.3} R4={:.3} R5={:.3} WebProp={:.4} H={}", r.j, r.auroc_r2, r.auroc_r3, r.auroc_r4, r.auroc_r5, r.web_prop, r.all_h_pass);
    }

    println!("\n--- RULE 5 CHAOS PROMOTION CHECK ---");
    for ord in ["OrderA", "OrderB", "OrderC"] {
        let base_n = format!("{ord}_NoChaos");
        let base_r = find(&results, &base_n);
        println!("{base_n}: J={:.4} (H={})", base_r.j, base_r.all_h_pass);
        for c in ["X1_Early", "X1_Mid", "X1_Late", "X2_Early", "X2_Mid", "X2_Late", "X1Early_X2Mid", "X1Mid_X2Late", "X1Early_X2Late"] {
            let cn = format!("{ord}_{c}");
            let cr = find(&results, &cn);
            let delta = cr.j - base_r.j;
            println!("  {cn:<26} J={:.4} (dJ={:+.4}) x1={:.4} x2={:.4} H={}", cr.j, delta, cr.x1_mean_spread, cr.x2_mean_spread, cr.all_h_pass);
        }
    }

    println!("\n--- RULE 6 ABLATIONS & NON-ATOM BASELINE ---");
    for n in [
        "Abl_NoMechanics",
        "Abl_NoPhysics",
        "Abl_NoInfo",
        "Abl_NoChemistry",
        "Abl_NoCognition",
        "Ctrl_DisconnectedWeb",
        "Baseline_NonAtom_Ewma",
    ] {
        let r = find(&results, n);
        println!("{n:<24} J={:.4} AUC_all={:.4} AUC_hard={:.4} WebProp={:.4} Recov={:.4} H={}", r.j, r.auroc_all, r.auroc_hard, r.web_prop, r.recovery, r.all_h_pass);
    }
}
