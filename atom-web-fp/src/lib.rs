#![no_std]

pub mod chaos;
pub mod chemistry;
pub mod cognition;
pub mod fixed;
pub mod info;
pub mod mechanics;
pub mod physics;
pub mod quantum;
pub mod topology;

pub use chaos::{ChaosReadout, LogisticChaos, StandardMapChaos};
pub use chemistry::{ChemReadout, ReactionKinetics};
pub use cognition::{CogReadout, PredictiveCognition};
pub use fixed::{clamp, div, exp_neg, isqrt, ln2, mul, mul_round, sin_cos, ComplexQ30, HALF, LN2, PI, QUARTER, SCALE, TWO_PI};
pub use info::{InfoChannel, InfoReadout};
pub use mechanics::{FluxReadout, HarmonicWeb, MechReadout, MomentumFlux};
pub use physics::{ThermalBath, ThermalReadout, T_AMBIENT, T_MIN};
pub use quantum::{ClassicalWalkBase, CoinedWebWalk, InterferometerReadout, PhaseInterferometer, QuantumReadout, UNIFORM_PROB};
pub use topology::{ccw, chord, cw, ARCS, BINS, STRANDS};

/// Raw per-event bus telemetry tuple fed into a strand of the web.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BusImpulse {
    pub strand: u8,
    pub bin: u8,
    pub magnitude: i64,
    pub dispersion: i64,
    pub phase_skew: i64,
    pub queue_pressure: i64,
    pub active: bool,
}
