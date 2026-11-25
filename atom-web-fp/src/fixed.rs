//! Deterministic Q30 fixed-point arithmetic (`SCALE = 1 << 30`, no floats).

pub const SCALE: i64 = 1i64 << 30;
pub const HALF: i64 = SCALE >> 1;
pub const QUARTER: i64 = SCALE >> 2;
pub const EIGHTH: i64 = SCALE >> 3;

pub const PI: i64 = 3_373_259_426;
pub const HALF_PI: i64 = 1_686_629_713;
pub const QUARTER_PI: i64 = 843_314_857;
pub const TWO_PI: i64 = 6_746_518_852;
pub const LN2: i64 = 744_261_118;
pub const INV_LN2: i64 = 1_549_082_005;

const SCALE128: i128 = 1i128 << 40;
const INV_LN2_Q40: i128 = 1_586_260_354_470;

#[inline]
pub const fn mul(a: i64, b: i64) -> i64 {
    ((a as i128 * b as i128) >> 30) as i64
}

#[inline]
pub const fn mul_round(a: i64, b: i64) -> i64 {
    let p = a as i128 * b as i128;
    if p >= 0 {
        ((p + (1i128 << 29)) >> 30) as i64
    } else {
        -(((-p + (1i128 << 29)) >> 30) as i64)
    }
}

#[inline]
pub const fn div(a: i64, b: i64) -> i64 {
    if b == 0 {
        0
    } else {
        (((a as i128) << 30) / (b as i128)) as i64
    }
}

#[inline]
pub const fn clamp(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[inline]
pub fn wrap_two_pi(angle: i64) -> i64 {
    let mut a = angle % TWO_PI;
    if a < 0 {
        a += TWO_PI;
    }
    a
}

/// Computes Q30 `sqrt(x)` for `x >= 0` using exact integer Newton iteration on `u128`.
pub fn isqrt(x: i64) -> i64 {
    if x <= 0 {
        return 0;
    }
    let n = (x as u128) << 30;
    let shift = (128 - n.leading_zeros()).div_ceil(2);
    let mut r = 1u128 << shift;
    loop {
        let y = (r + n / r) >> 1;
        if y >= r {
            return r as i64;
        }
        r = y;
    }
}

fn poly_sin_cos_octant(u: i64) -> (i64, i64) {
    let u40 = (u as i128) << 10;
    let u2 = (u40 * u40) >> 40;

    let mut s_acc = u40;
    let mut term = u40;
    term = -((term * u2) >> 40) / 6;
    s_acc += term;
    term = -((term * u2) >> 40) / 20;
    s_acc += term;
    term = -((term * u2) >> 40) / 42;
    s_acc += term;
    term = -((term * u2) >> 40) / 72;
    s_acc += term;
    term = -((term * u2) >> 40) / 110;
    s_acc += term;

    let mut c_acc = SCALE128;
    let mut cterm = SCALE128;
    cterm = -((cterm * u2) >> 40) / 2;
    c_acc += cterm;
    cterm = -((cterm * u2) >> 40) / 12;
    c_acc += cterm;
    cterm = -((cterm * u2) >> 40) / 30;
    c_acc += cterm;
    cterm = -((cterm * u2) >> 40) / 56;
    c_acc += cterm;
    cterm = -((cterm * u2) >> 40) / 90;
    c_acc += cterm;

    let s = ((s_acc + 512) >> 10) as i64;
    let c = ((c_acc + 512) >> 10) as i64;
    (s, c)
}

/// Returns Q30 `(sin(angle), cos(angle))` with norm polished to `<= 4e-9`.
pub fn sin_cos(angle: i64) -> (i64, i64) {
    let a = wrap_two_pi(angle);
    let q = a / HALF_PI;
    let r = a - q * HALF_PI;
    let (so, co) = if r <= QUARTER_PI {
        poly_sin_cos_octant(r)
    } else {
        let (su, cu) = poly_sin_cos_octant(HALF_PI - r);
        (cu, su)
    };
    let (mut s, mut c) = match q {
        0 => (so, co),
        1 => (co, -so),
        2 => (-so, -co),
        _ => (-co, so),
    };
    if s != 0 && c != 0 {
        let norm_sq = mul_round(s, s) + mul_round(c, c);
        let norm = isqrt(norm_sq);
        if norm > 0 {
            s = div(s, norm);
            c = div(c, norm);
        }
    }
    (s, c)
}

/// Computes Q30 `log2(x)` for positive Q30 `x`.
pub fn ln2(x: i64) -> i64 {
    if x <= 0 {
        return -30 * SCALE;
    }
    let mut m = x;
    let mut exp: i64 = 0;
    while m >= 2 * SCALE {
        m >>= 1;
        exp += 1;
    }
    while m < SCALE {
        m <<= 1;
        exp -= 1;
    }
    let u40 = (((m - SCALE) as i128) << 40) / ((m + SCALE) as i128);
    let u2 = (u40 * u40) >> 40;
    let mut pow = u40;
    let mut sum = pow;
    for denom in [3i128, 5, 7, 9, 11, 13] {
        pow = (pow * u2) >> 40;
        sum += pow / denom;
    }
    let ln_m40 = sum << 1;
    let log2_m40 = (ln_m40 * INV_LN2_Q40) >> 40;
    let log2_m = ((log2_m40 + 512) >> 10) as i64;
    exp * SCALE + log2_m
}

/// Computes Q30 `exp(-x)` for `x >= 0` (`SCALE` for `x <= 0`, `0` for `x >= 24*SCALE`).
pub fn exp_neg(x: i64) -> i64 {
    if x <= 0 {
        return SCALE;
    }
    if x >= 24 * SCALE {
        return 0;
    }
    let n = (x / LN2) as u32;
    let r = x - (n as i64) * LN2;
    let r40 = (r as i128) << 10;
    let mut acc = SCALE128;
    let mut term = SCALE128;
    for k in 1i128..=10 {
        term = -((term * r40) >> 40) / k;
        acc += term;
    }
    let shift = 10 + n;
    if shift >= 64 {
        0
    } else {
        ((acc + (1i128 << (shift - 1))) >> shift).max(0) as i64
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ComplexQ30 {
    pub re: i64,
    pub im: i64,
}

impl ComplexQ30 {
    #[inline]
    pub const fn new(re: i64, im: i64) -> Self {
        Self { re, im }
    }

    #[inline]
    pub const fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    #[inline]
    pub const fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    #[inline]
    pub const fn mul_unit(self, cos_a: i64, sin_a: i64) -> Self {
        Self {
            re: mul_round(self.re, cos_a) - mul_round(self.im, sin_a),
            im: mul_round(self.re, sin_a) + mul_round(self.im, cos_a),
        }
    }

    #[inline]
    pub const fn norm_sq(self) -> i64 {
        mul_round(self.re, self.re) + mul_round(self.im, self.im)
    }

    #[inline]
    pub const fn scale(self, factor: i64) -> Self {
        Self {
            re: mul_round(self.re, factor),
            im: mul_round(self.im, factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_circle_norm_and_accuracy() {
        let mut max_norm_err = 0i64;
        let mut max_trig_err = 0.0f64;
        for step in -500..=500 {
            let ang_f = (step as f64) * 0.021;
            let ang_q = (ang_f * (SCALE as f64)) as i64;
            let (s, c) = sin_cos(ang_q);
            let norm_sq = mul_round(s, s) + mul_round(c, c);
            let err = (norm_sq - SCALE).abs();
            if err > max_norm_err {
                max_norm_err = err;
            }
            let sf = (s as f64) / (SCALE as f64);
            let cf = (c as f64) / (SCALE as f64);
            let e = (sf - ang_f.sin()).abs().max((cf - ang_f.cos()).abs());
            if e > max_trig_err {
                max_trig_err = e;
            }
        }
        assert!(max_norm_err <= 12, "unit norm err too big: {max_norm_err}");
        assert!(max_trig_err < 1e-8, "trig err too big: {max_trig_err}");
    }

    #[test]
    fn ln2_and_exp_neg_accuracy() {
        for &val in &[0.01f64, 0.125, 0.5, 0.99, 1.0, 1.5, 2.0, 7.25, 64.0] {
            let q = (val * (SCALE as f64)) as i64;
            let got = (ln2(q) as f64) / (SCALE as f64);
            let expect = val.log2();
            assert!((got - expect).abs() < 1e-6, "ln2({val})={got} != {expect}");
        }
        for &val in &[0.0f64, 0.1, 0.5, 0.693, 1.0, 2.5, 6.0, 12.0] {
            let q = (val * (SCALE as f64)) as i64;
            let got = (exp_neg(q) as f64) / (SCALE as f64);
            let expect = (-val).exp();
            assert!((got - expect).abs() < 1e-6, "exp(-{val})={got} != {expect}");
        }
    }

    #[test]
    fn isqrt_accuracy() {
        for &val in &[0.0f64, 0.001, 0.25, 1.0, 2.0, 17.5, 250.0] {
            let q = (val * (SCALE as f64)) as i64;
            let got = (isqrt(q) as f64) / (SCALE as f64);
            let expect = ((q as f64) / (SCALE as f64)).sqrt();
            assert!((got - expect).abs() < 1e-8, "isqrt({val})={got} != {expect}");
        }
    }
}
