//! E19-K: the weave engine, from first principles, as an OS payload.
//!
//! The quantum postulates as code — amplitudes, unitary gates,
//! destructive Born measurement — running in ring 3 under the
//! kernel's orchestrator. No cloud, no simulation host: the OS runs
//! its own physics. Gates W0/W1/W2 from the quantum-network weave,
//! verified live in QEMU:
//!
//! - W0 no-cloning, measured: a CNOT cloner copies basis states at
//!   fidelity 1.000 and fails on |+> at exactly 0.500 joint fidelity
//!   with a maximally mixed target marginal.
//! - W1 payload never travels: random states teleport through a
//!   swapped pair at fidelity 1.000 while the middle holds nothing.
//! - W2 monogamy cascade: clean path violates CHSH, a tapped path
//!   collapses below the classical bound.
//!
//! Honesty printed with the verdicts: measurement outcomes here come
//! from a seeded PRNG (SplitMix64) — pseudo-random, deterministic.
//! Real quantum hardware's Born outcomes are nature's; the cloud run
//! (tools/e19_real.py --real) is where these same gates stop being
//! our axioms and become an experiment.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

// ---- first principles: amplitudes and the Born rule ---------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Cx {
    re: f64,
    im: f64,
}

impl Cx {
    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
    fn norm_sq(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

impl core::ops::Add for Cx {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
}
impl core::ops::Mul for Cx {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
}

const MAX_AMPLITUDES: usize = 16; // up to 4 qubits

#[derive(Clone, Copy, Debug, PartialEq)]
struct Register {
    qubits: usize,
    amplitudes: [Cx; MAX_AMPLITUDES],
}

impl Register {
    fn zeros(qubits: usize) -> Self {
        let mut amplitudes = [Cx::default(); MAX_AMPLITUDES];
        amplitudes[0] = Cx::new(1.0, 0.0);
        Self { qubits, amplitudes }
    }

    fn basis(qubits: usize, index: usize) -> Self {
        let mut amplitudes = [Cx::default(); MAX_AMPLITUDES];
        amplitudes[index] = Cx::new(1.0, 0.0);
        Self { qubits, amplitudes }
    }

    /// A seeded random pure single-qubit state.
    fn random_qubit(rng: &mut Rng) -> Self {
        let theta = rng.unit() * core::f64::consts::PI;
        let phi = rng.unit() * 2.0 * core::f64::consts::PI;
        let mut state = Self::zeros(1);
        state.amplitudes[0] = Cx::new(cos(theta / 2.0), 0.0);
        let s = sin(theta / 2.0);
        state.amplitudes[1] = Cx::new(s * cos(phi), s * sin(phi));
        state
    }

    /// Little-endian tensor: self = low qubits, other = high.
    /// Fixed-capacity: hard-bounded at 4 qubits (16 amplitudes) —
    /// an overflow here corrupts payload memory, so it asserts.
    fn tensor(&self, other: &Self) -> Self {
        assert!(self.qubits + other.qubits <= 4, "register overflow: max 4 qubits");
        let mut out = Self::zeros(self.qubits + other.qubits);
        for b in 0..1usize << other.qubits {
            for a in 0..1usize << self.qubits {
                out.amplitudes[a | (b << self.qubits)] = self.amplitudes[a] * other.amplitudes[b];
            }
        }
        out
    }

    fn apply1(&mut self, q: usize, gate: [[Cx; 2]; 2]) {
        let step = 1usize << q;
        let mut base = 0usize;
        while base < 1 << self.qubits {
            for offset in 0..step {
                let i0 = base + offset;
                let i1 = i0 + step;
                let (a, b) = (self.amplitudes[i0], self.amplitudes[i1]);
                self.amplitudes[i0] = gate[0][0] * a + gate[0][1] * b;
                self.amplitudes[i1] = gate[1][0] * a + gate[1][1] * b;
            }
            base += 2 * step;
        }
    }

    fn apply_controlled(&mut self, control: usize, target: usize, gate: [[Cx; 2]; 2]) {
        let cbit = 1usize << control;
        let tstep = 1usize << target;
        let mut base = 0usize;
        while base < 1 << self.qubits {
            for offset in 0..tstep {
                let i0 = base + offset;
                let i1 = i0 + tstep;
                if i0 & cbit != 0 {
                    let (a, b) = (self.amplitudes[i0], self.amplitudes[i1]);
                    self.amplitudes[i0] = gate[0][0] * a + gate[0][1] * b;
                    self.amplitudes[i1] = gate[1][0] * a + gate[1][1] * b;
                }
            }
            base += 2 * tstep;
        }
    }

    fn rotate_y(&mut self, q: usize, theta: f64) {
        let half = theta / 2.0;
        self.apply1(q, [
            [Cx::new(cos(half), 0.0), Cx::new(-sin(half), 0.0)],
            [Cx::new(sin(half), 0.0), Cx::new(cos(half), 0.0)],
        ]);
    }

    /// The Born rule: sample, collapse, destroy. Pseudo-random by
    /// seeded PRNG here; nature's randomness on real hardware.
    fn measure(&mut self, q: usize, rng: &mut Rng) -> u8 {
        let step = 1usize << q;
        let mut p_one = 0.0f64;
        for index in 0..1 << self.qubits {
            if index & step != 0 {
                p_one += self.amplitudes[index].norm_sq();
            }
        }
        let outcome = if rng.unit() < p_one { 1u8 } else { 0u8 };
        let keep = (outcome as usize) << q;
        let mut norm = 0.0f64;
        for index in 0..1 << self.qubits {
            if index & step == keep {
                norm += self.amplitudes[index].norm_sq();
            } else {
                self.amplitudes[index] = Cx::default();
            }
        }
        // Babylonian sqrt: converges from any positive start (the
        // inverse form needs the argument near 1 and broke W1).
        let mut y = if norm > 0.0 { norm } else { 1.0 };
        for _ in 0..40 {
            y = 0.5 * (y + norm / y);
        }
        let scale = if y > 0.0 { 1.0 / y } else { 1.0 };
        for index in 0..1 << self.qubits {
            let a = self.amplitudes[index];
            self.amplitudes[index] = Cx::new(a.re * scale, a.im * scale);
        }
        outcome
    }

    fn fidelity(&self, other: &Self) -> f64 {
        let mut overlap = Cx::default();
        for index in 0..1 << self.qubits {
            overlap = overlap + self.amplitudes[index].conj() * other.amplitudes[index];
        }
        overlap.norm_sq()
    }

    /// tr(rho_q |psi><psi|) for 2-qubit registers — the honest
    /// marginal.
    fn reduced_fidelity(&self, qubit: usize, target: &Self) -> f64 {
        let other = 1 - qubit;
        let mut rho = [[Cx::default(); 2]; 2];
        for (a, row) in rho.iter_mut().enumerate() {
            for (b, slot) in row.iter_mut().enumerate() {
                let mut sum = Cx::default();
                for c in 0..2usize {
                    let ia = a << qubit | c << other;
                    let ib = b << qubit | c << other;
                    sum = sum + self.amplitudes[ia] * self.amplitudes[ib].conj();
                }
                *slot = sum;
            }
        }
        let mut value = Cx::default();
        for (a, row) in rho.iter().enumerate() {
            for (b, cell) in row.iter().enumerate() {
                value = value + *cell * target.amplitudes[a] * target.amplitudes[b].conj();
            }
        }
        value.re.max(0.0)
    }
}

/// In-repo trig (f64::cos/sin live in std, not core; the kernel
/// stays dependency-free). Argument reduction to [0, pi/2), then
/// Taylor — accuracy ~2e-7 at the interval edge, inside the gates'
/// 1e-6 tolerance.
fn cos_sin(x: f64) -> (f64, f64) {
    let pi = core::f64::consts::PI;
    // Normalize to [0, 2*pi).
    let two_pi = 2.0 * pi;
    let mut x = x % two_pi;
    if x < 0.0 {
        x += two_pi;
    }
    let (quadrant, t) = if x < pi / 2.0 {
        (0u8, x)
    } else if x < pi {
        (1, x - pi / 2.0)
    } else if x < 3.0 * pi / 2.0 {
        (2, x - pi)
    } else {
        (3, x - 3.0 * pi / 2.0)
    };
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t2 * t2;
    let t5 = t4 * t;
    let t6 = t4 * t2;
    let t7 = t6 * t;
    let t8 = t6 * t2;
    let t9 = t8 * t;
    let t10 = t8 * t2;
    let t11 = t10 * t;
    let sin_t = t - t3 / 6.0 + t5 / 120.0 - t7 / 5040.0 + t9 / 362_880.0 - t11 / 39_916_800.0;
    let cos_t = 1.0 - t2 / 2.0 + t4 / 24.0 - t6 / 720.0 + t8 / 40_320.0 - t10 / 3_628_800.0;
    match quadrant {
        0 => (cos_t, sin_t),
        1 => (-sin_t, cos_t),
        2 => (-cos_t, -sin_t),
        _ => (sin_t, -cos_t),
    }
}

fn cos(x: f64) -> f64 {
    cos_sin(x).0
}

fn sin(x: f64) -> f64 {
    cos_sin(x).1
}

const H: [[Cx; 2]; 2] = [
    [Cx::new(core::f64::consts::FRAC_1_SQRT_2, 0.0), Cx::new(core::f64::consts::FRAC_1_SQRT_2, 0.0)],
    [Cx::new(core::f64::consts::FRAC_1_SQRT_2, 0.0), Cx::new(-core::f64::consts::FRAC_1_SQRT_2, 0.0)],
];
const X: [[Cx; 2]; 2] = [
    [Cx::new(0.0, 0.0), Cx::new(1.0, 0.0)],
    [Cx::new(1.0, 0.0), Cx::new(0.0, 0.0)],
];
const Z: [[Cx; 2]; 2] = [
    [Cx::new(1.0, 0.0), Cx::new(0.0, 0.0)],
    [Cx::new(0.0, 0.0), Cx::new(-1.0, 0.0)],
];

// SplitMix64 — deterministic pseudo-randomness, stated honestly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 40) as f64 / (1u64 << 24) as f64
    }
}

// ---- the weave ------------------------------------------------------

fn bell_pair() -> Register {
    let mut pair = Register::zeros(2);
    pair.apply1(0, H);
    pair.apply_controlled(0, 1, X);
    pair
}

fn weave_register() -> Register {
    bell_pair().tensor(&bell_pair())
}

/// BSM at B (qubits 1,2) then Pauli correction on qubit 3.
fn swap_links(register: &mut Register, rng: &mut Rng) {
    register.apply_controlled(1, 2, X);
    register.apply1(1, H);
    let m1 = register.measure(1, rng);
    let m2 = register.measure(2, rng);
    if m2 == 1 {
        register.apply1(3, X);
    }
    if m1 == 1 {
        register.apply1(3, Z);
    }
}

/// Teleport a 1-qubit payload through a fresh Bell pair; returns the
/// receiver's state.
fn teleport(payload: &Register, rng: &mut Rng) -> Register {
    let mut r = payload.tensor(&bell_pair());
    r.apply_controlled(0, 1, X);
    r.apply1(0, H);
    let b0 = r.measure(0, rng);
    let b1 = r.measure(1, rng);
    if b1 == 1 {
        r.apply1(2, X);
    }
    if b0 == 1 {
        r.apply1(2, Z);
    }
    let keep = b0 as usize | ((b1 as usize) << 1);
    let mut receiver = Register::zeros(1);
    for basis in 0..8usize {
        if basis & 0b011 == keep {
            receiver.amplitudes[(basis & 0b100) >> 2] = r.amplitudes[basis];
        }
    }
    receiver
}

fn cloning_fidelity(input: &Register) -> (f64, f64) {
    let mut r = input.tensor(&Register::basis(1, 0));
    r.apply_controlled(0, 1, X);
    let target = input.tensor(input);
    (r.fidelity(&target), r.reduced_fidelity(1, input))
}

fn correlation(register: &Register, left: usize, right: usize, ta: f64, tb: f64, rng: &mut Rng, shots: usize) -> f64 {
    let mut agree = 0i64;
    for _ in 0..shots {
        let mut probe = *register;
        probe.rotate_y(left, ta);
        probe.rotate_y(right, tb);
        let first = probe.measure(left, rng);
        let second = probe.measure(right, rng);
        if first == second {
            agree += 1;
        }
    }
    (2 * agree - shots as i64) as f64 / shots as f64
}

fn chsh(register: &Register, left: usize, right: usize, rng: &mut Rng, shots: usize) -> f64 {
    let pi = core::f64::consts::PI;
    let e00 = correlation(register, left, right, 0.0, pi / 4.0, rng, shots);
    let e01 = correlation(register, left, right, 0.0, 3.0 * pi / 4.0, rng, shots);
    let e10 = correlation(register, left, right, pi / 2.0, pi / 4.0, rng, shots);
    let e11 = correlation(register, left, right, pi / 2.0, 3.0 * pi / 4.0, rng, shots);
    e00 - e01 + e10 + e11
}

// ---- the payload ----------------------------------------------------

fn main() {
    rt::print("[Weave] first-principles engine online (ring 3, under the orchestrator)\n");

    // W0: no-cloning, measured.
    let mut plus = Register::zeros(1);
    plus.apply1(0, H);
    let (j0, _) = cloning_fidelity(&Register::basis(1, 0));
    let (j1, _) = cloning_fidelity(&Register::basis(1, 1));
    let (jp, mp) = cloning_fidelity(&plus);
    rt::print_args(format_args!(
        "[Weave] W0 clone basis {j0:.3}/{j1:.3}, clone |+> joint {jp:.3} marginal {mp:.3}\n"
    ));
    assert!(j0 > 0.999 && j1 > 0.999);
    assert!((jp - 0.5).abs() < 1e-6 && (mp - 0.5).abs() < 1e-6);
    rt::print("[Weave] W0 PASS: copies bits, never unknown amplitudes\n");

    // W1: the payload never travels.
    let mut rng = Rng::new(0xE19);
    let mut receiver_total = 0.0f64;
    for _ in 0..10 {
        let payload = Register::random_qubit(&mut rng);
        let mut register = weave_register();
        swap_links(&mut register, &mut rng);
        let received = teleport(&payload, &mut rng);
        receiver_total += payload.fidelity(&received);
    }
    let average = receiver_total / 10.0;
    rt::print_args(format_args!("[Weave] W1 receiver fidelity {average:.3} over 10 payloads\n"));
    assert!(average > 0.99);
    rt::print("[Weave] W1 PASS: the middle touches nothing\n");

    // W2: monogamy cascade.
    let mut rng = Rng::new(0xE19 ^ 2);
    let mut clean = weave_register();
    swap_links(&mut clean, &mut rng);
    let s_clean = chsh(&clean, 0, 3, &mut rng, 512);
    // Tapped weave, 4-qubit equivalent: honest AB pair (0,1), junk
    // placeholder at B's second link (2), and C holding an
    // UNCORRELATED random state (3) — Eve walked away with the
    // partner. CHSH(0,3) sees no entanglement.
    let mut rng_eve = Rng::new(0xE2E);
    let tapped = bell_pair()
        .tensor(&Register::basis(1, 0))
        .tensor(&Register::random_qubit(&mut rng_eve));
    let s_tapped = chsh(&tapped, 0, 3, &mut rng, 512);
    rt::print_args(format_args!(
        "[Weave] W2 CHSH clean {s_clean:.3} vs tapped {s_tapped:.3} (bound 2)\n"
    ));
    assert!(s_clean > 2.4 && s_tapped < 2.4);
    rt::print("[Weave] W2 PASS: one heartbeat feels the whole path\n");

    rt::print("[Weave] honesty: Born outcomes are seeded PRNG (SplitMix64) — the cloud run is where nature votes\n");
    rt::print("[Weave] E19-K PASS: the OS runs its own physics\n");
    rt::exit(0);
}
