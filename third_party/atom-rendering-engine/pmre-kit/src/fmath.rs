//! `f32` methods that live in `std` rather than `core`, for `no_std` builds. Brought into
//! scope through the prelude only without `std`, where the inherent methods are absent, so
//! call sites read the same either way. Accuracy is ample for rasterization.

pub trait F32Ext {
    fn floor(self) -> f32;
    fn ceil(self) -> f32;
    fn round(self) -> f32;
    fn trunc(self) -> f32;
    fn sqrt(self) -> f32;
    fn sin(self) -> f32;
    fn cos(self) -> f32;
    fn rem_euclid(self, rhs: f32) -> f32;
}

/// Above 2^23 every `f32` is already an integer.
const INTEGRAL: f32 = 8_388_608.0;

impl F32Ext for f32 {
    fn trunc(self) -> f32 {
        if self.is_nan() || self.abs() >= INTEGRAL {
            self
        } else {
            self as i32 as f32
        }
    }
    fn floor(self) -> f32 {
        let t = F32Ext::trunc(self);
        if t > self {
            t - 1.0
        } else {
            t
        }
    }
    fn ceil(self) -> f32 {
        let t = F32Ext::trunc(self);
        if t < self {
            t + 1.0
        } else {
            t
        }
    }
    /// Half away from zero, like `std`.
    fn round(self) -> f32 {
        if self >= 0.0 {
            F32Ext::floor(self + 0.5)
        } else {
            -F32Ext::floor(-self + 0.5)
        }
    }
    fn sqrt(self) -> f32 {
        #[cfg(target_arch = "x86_64")]
        {
            use core::arch::x86_64::{_mm_cvtss_f32, _mm_set_ss, _mm_sqrt_ss};
            unsafe { _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(self))) }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            if self <= 0.0 || self.is_nan() || self.is_infinite() {
                return if self == 0.0 || self.is_infinite() {
                    self
                } else {
                    f32::NAN
                };
            }
            // Bit-level initial guess, then Newton iterations.
            let mut y = f32::from_bits((self.to_bits() >> 1) + 0x1fbd_1df5);
            for _ in 0..4 {
                y = 0.5 * (y + self / y);
            }
            y
        }
    }
    fn sin(self) -> f32 {
        use core::f32::consts::{PI, TAU};
        let mut x = self - F32Ext::floor(self / TAU) * TAU; // [0, 2π)
        if x > PI {
            x -= TAU; // (-π, π]
        }
        // Fold into [-π/2, π/2] where the series converges quickly.
        if x > PI / 2.0 {
            x = PI - x;
        } else if x < -PI / 2.0 {
            x = -PI - x;
        }
        let x2 = x * x;
        x * (1.0
            - x2 / 6.0
                * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
    }
    fn cos(self) -> f32 {
        F32Ext::sin(self + core::f32::consts::FRAC_PI_2)
    }
    fn rem_euclid(self, rhs: f32) -> f32 {
        let r = self - F32Ext::trunc(self / rhs) * rhs;
        if r < 0.0 {
            r + rhs.abs()
        } else {
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::F32Ext;

    #[test]
    fn matches_std_on_representative_values() {
        for &v in &[
            -3.7f32, -2.5, -1.0, -0.4, 0.0, 0.4, 0.5, 1.5, 2.49, 7.0, 1e9, -1e9,
        ] {
            assert_eq!(F32Ext::floor(v), v.floor(), "floor {v}");
            assert_eq!(F32Ext::ceil(v), v.ceil(), "ceil {v}");
            assert_eq!(F32Ext::round(v), v.round(), "round {v}");
            assert_eq!(F32Ext::trunc(v), v.trunc(), "trunc {v}");
        }
        for &v in &[0.0f32, 1e-6, 0.25, 2.0, 3.0, 1e6] {
            assert!(
                (F32Ext::sqrt(v) - v.sqrt()).abs() <= v.sqrt() * 1e-6 + 1e-9,
                "sqrt {v}"
            );
        }
        for i in -40..=40 {
            let a = i as f32 * 0.37;
            assert!((F32Ext::sin(a) - a.sin()).abs() < 2e-4, "sin {a}");
            assert!((F32Ext::cos(a) - a.cos()).abs() < 2e-4, "cos {a}");
        }
        assert_eq!(F32Ext::rem_euclid(-30.0, 360.0), 330.0);
        assert_eq!(F32Ext::rem_euclid(725.0, 360.0), 5.0);
    }
}
