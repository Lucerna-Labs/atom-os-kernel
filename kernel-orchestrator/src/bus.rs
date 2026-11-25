//! Outer conventional Ring-0 L0 Spiderweb Bus substrate driving the promoted
//! observation-only Atom stack (`OrderB_X1_Early`) at 100 Hz tick cadence.

use atom_web_engine::{ChaosPos, StackConfig, StackOrder, WebEngine, WebObservation, NUM_SCALARS};
use atom_web_fp::{clamp, BusImpulse, PI, SCALE, STRANDS, T_AMBIENT};
use kernel_kit::abi::{WebStats, MAX_PROCESSES, WEB_CONFIG_ORDERB_X1EARLY};

pub const PROMOTED_CONFIG: StackConfig =
    StackConfig::full(StackOrder::OrderB).with_chaos(ChaosPos::Early, ChaosPos::Off);

#[derive(Clone, Copy, Debug)]
struct StrandAccum {
    events: u32,
    mag_q30: i64,
    pressure_q30: i64,
    last_bin: u8,
    bin_switches: u16,
    last_event_tick: u64,
    prev_dt: u64,
    skew_acc: i64,
    has_seen: bool,
    active: bool,
}

impl StrandAccum {
    const fn new() -> Self {
        Self {
            events: 0,
            mag_q30: 0,
            pressure_q30: 0,
            last_bin: 0,
            bin_switches: 0,
            last_event_tick: 0,
            prev_dt: 0,
            skew_acc: 0,
            has_seen: false,
            active: false,
        }
    }
}

pub struct SpiderwebBus {
    events_total: u64,
    accum: [StrandAccum; STRANDS],
    engine: WebEngine,
    strand_gen: [u64; STRANDS],
    last_obs: WebObservation,
    steps: u64,
    max_norm_err_q30: i64,
    max_chem_err_q30: i64,
    max_antisym_err_q30: i64,
    min_temp_q30: i64,
}

impl Default for SpiderwebBus {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiderwebBus {
    pub const fn new() -> Self {
        Self {
            events_total: 0,
            accum: [StrandAccum::new(); STRANDS],
            engine: WebEngine::new(PROMOTED_CONFIG),
            strand_gen: [0; STRANDS],
            last_obs: WebObservation {
                scalars: [0; NUM_SCALARS],
                strand_response: [0; STRANDS],
                active_obs_mask: PROMOTED_CONFIG.active_obs_mask(),
                unitary_norm_err: 0,
                chem_mass_err: 0,
                antisym_err: 0,
                min_temp: T_AMBIENT,
                min_entropy_prod: 0,
                min_species: SCALE,
                x1_spread: 0,
                x2_spread: 0,
            },
            steps: 0,
            max_norm_err_q30: 0,
            max_chem_err_q30: 0,
            max_antisym_err_q30: 0,
            min_temp_q30: T_AMBIENT,
        }
    }

    pub fn on_strand_spawn(&mut self, slot: usize, now_tick: u64) {
        if slot >= STRANDS {
            return;
        }
        self.strand_gen[slot] = self.strand_gen[slot].wrapping_add(1);
        self.record_event(slot, 5, SCALE >> 2, 0, now_tick);
    }

    pub fn on_strand_exit(&mut self, slot: usize, now_tick: u64) {
        if slot >= STRANDS {
            return;
        }
        self.record_event(slot, 5, SCALE >> 2, 0, now_tick);
    }

    #[inline(always)]
    pub fn record_event(
        &mut self,
        slot: usize,
        bin: u8,
        mag_q30: i64,
        pressure_q30: i64,
        now_tick: u64,
    ) {
        if slot >= STRANDS {
            return;
        }
        self.events_total = self.events_total.wrapping_add(1);
        let acc = &mut self.accum[slot];
        let b = bin & 7;
        if acc.has_seen && acc.last_bin != b {
            acc.bin_switches = acc.bin_switches.saturating_add(1);
        }
        acc.has_seen = true;
        acc.last_bin = b;
        acc.events = acc.events.saturating_add(1);
        acc.mag_q30 = clamp(acc.mag_q30.saturating_add(mag_q30.max(0)), 0, 16 * SCALE);
        if pressure_q30 > acc.pressure_q30 {
            acc.pressure_q30 = pressure_q30.min(16 * SCALE);
        }
        if now_tick != acc.last_event_tick {
            let dt = now_tick.saturating_sub(acc.last_event_tick);
            let delta = if dt >= acc.prev_dt {
                ((dt - acc.prev_dt).min(8) as i64) * (SCALE >> 4)
            } else {
                -(((acc.prev_dt - dt).min(8) as i64) * (SCALE >> 4))
            };
            acc.skew_acc = clamp(acc.skew_acc + delta, -PI, PI);
            acc.prev_dt = dt;
            acc.last_event_tick = now_tick;
        }
        acc.active = true;
    }

    pub fn advance_tick(&mut self, _now_tick: u64) {
        let mut impulses = [BusImpulse::default(); STRANDS];
        for s in 0..STRANDS {
            let acc = &mut self.accum[s];
            impulses[s] = BusImpulse {
                strand: s as u8,
                bin: acc.last_bin,
                magnitude: acc.mag_q30,
                dispersion: clamp((acc.bin_switches as i64) * (SCALE >> 3), 0, 16 * SCALE),
                phase_skew: acc.skew_acc,
                queue_pressure: acc.pressure_q30,
                active: acc.active,
            };
            acc.events = 0;
            acc.mag_q30 = 0;
            acc.pressure_q30 = 0;
            acc.bin_switches = 0;
            acc.skew_acc = 0;
            acc.active = false;
        }

        let obs = self.engine.step(&impulses);
        self.steps = self.steps.wrapping_add(1);
        if obs.unitary_norm_err > self.max_norm_err_q30 {
            self.max_norm_err_q30 = obs.unitary_norm_err;
        }
        if obs.chem_mass_err > self.max_chem_err_q30 {
            self.max_chem_err_q30 = obs.chem_mass_err;
        }
        if obs.antisym_err > self.max_antisym_err_q30 {
            self.max_antisym_err_q30 = obs.antisym_err;
        }
        if obs.min_temp < self.min_temp_q30 {
            self.min_temp_q30 = obs.min_temp;
        }
        self.last_obs = obs;
    }

    pub fn snapshot(&self) -> WebStats {
        let mut stats = WebStats {
            steps: self.steps,
            events_total: self.events_total,
            active_obs_mask: self.last_obs.active_obs_mask as u32,
            config_tag: WEB_CONFIG_ORDERB_X1EARLY,
            max_norm_err_q30: self.max_norm_err_q30,
            max_chem_err_q30: self.max_chem_err_q30,
            max_antisym_err_q30: self.max_antisym_err_q30,
            min_temp_q30: self.min_temp_q30,
            x1_spread_q30: self.last_obs.x1_spread,
            scalars_q30: self.last_obs.scalars,
            strand_response_q30: [0; MAX_PROCESSES],
            strand_gen: [0; MAX_PROCESSES],
        };
        let n = MAX_PROCESSES.min(STRANDS);
        stats.strand_response_q30[..n].copy_from_slice(&self.last_obs.strand_response[..n]);
        stats.strand_gen[..n].copy_from_slice(&self.strand_gen[..n]);
        stats
    }
}
