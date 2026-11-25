#![no_std]

pub use atom_web_fp;
use atom_web_fp::fixed::HALF_PI;
use atom_web_fp::{
    ccw, chord, clamp, cw, BusImpulse, ClassicalWalkBase, CoinedWebWalk, HarmonicWeb, InfoChannel,
    LogisticChaos, MomentumFlux, PhaseInterferometer, PredictiveCognition, ReactionKinetics,
    StandardMapChaos, ThermalBath, PI, SCALE, STRANDS, T_AMBIENT, UNIFORM_PROB,
};

pub const NUM_SCALARS: usize = 12;

pub const OBS_QUANTUM_CONTRAST: u16 = 1 << 0;
pub const OBS_QUANTUM_BORN: u16 = 1 << 1;
pub const OBS_CHIRAL_CURRENT: u16 = 1 << 2;
pub const OBS_MECH_ENERGY: u16 = 1 << 3;
pub const OBS_MECH_FLUX: u16 = 1 << 4;
pub const OBS_THERMAL_SIGMA: u16 = 1 << 5;
pub const OBS_THERMAL_EXCESS: u16 = 1 << 6;
pub const OBS_INFO_KL: u16 = 1 << 7;
pub const OBS_INFO_SURPRISE: u16 = 1 << 8;
pub const OBS_CHEM_B: u16 = 1 << 9;
pub const OBS_CHEM_C: u16 = 1 << 10;
pub const OBS_COG_FE: u16 = 1 << 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackOrder {
    OrderA,
    OrderB,
    OrderC,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseMode {
    QuantumFull,
    QuantumNoInterferometer,
    ClassicalWalkReplacement,
    NoBase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChaosPos {
    Off,
    Early,
    Mid,
    Late,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackConfig {
    pub order: StackOrder,
    pub base: BaseMode,
    pub mechanics: bool,
    pub physics: bool,
    pub info: bool,
    pub chemistry: bool,
    pub cognition: bool,
    pub connected: bool,
    pub x1: ChaosPos,
    pub x2: ChaosPos,
}

impl StackConfig {
    pub const fn full(order: StackOrder) -> Self {
        Self {
            order,
            base: BaseMode::QuantumFull,
            mechanics: true,
            physics: true,
            info: true,
            chemistry: true,
            cognition: true,
            connected: true,
            x1: ChaosPos::Off,
            x2: ChaosPos::Off,
        }
    }

    pub const fn with_chaos(mut self, x1: ChaosPos, x2: ChaosPos) -> Self {
        self.x1 = x1;
        self.x2 = x2;
        self
    }

    pub const fn active_obs_mask(&self) -> u16 {
        let mut m = 0u16;
        match self.base {
            BaseMode::QuantumFull => {
                m |= OBS_QUANTUM_CONTRAST | OBS_QUANTUM_BORN | OBS_CHIRAL_CURRENT;
            }
            BaseMode::QuantumNoInterferometer => {
                m |= OBS_QUANTUM_CONTRAST | OBS_QUANTUM_BORN;
            }
            BaseMode::ClassicalWalkReplacement => {
                m |= OBS_QUANTUM_BORN;
            }
            BaseMode::NoBase => {}
        }
        if self.mechanics {
            m |= OBS_MECH_ENERGY;
            if self.connected {
                m |= OBS_MECH_FLUX;
            }
        }
        if self.physics {
            m |= OBS_THERMAL_SIGMA | OBS_THERMAL_EXCESS;
        }
        if self.info {
            m |= OBS_INFO_KL | OBS_INFO_SURPRISE;
        }
        if self.chemistry {
            m |= OBS_CHEM_B | OBS_CHEM_C;
        }
        if self.cognition {
            m |= OBS_COG_FE;
        }
        m
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebObservation {
    pub scalars: [i64; NUM_SCALARS],
    pub strand_response: [i64; STRANDS],
    pub active_obs_mask: u16,
    pub unitary_norm_err: i64,
    pub chem_mass_err: i64,
    pub antisym_err: i64,
    pub min_temp: i64,
    pub min_entropy_prod: i64,
    pub min_species: i64,
    pub x1_spread: i64,
    pub x2_spread: i64,
}

struct StepScratch {
    active_mask: u16,
    bins: [u8; STRANDS],
    mag: [i64; STRANDS],
    disp: [i64; STRANDS],
    skew: [i64; STRANDS],
    press: [i64; STRANDS],
    early_chaos: [i64; STRANDS],

    probs: [i64; STRANDS],
    contrast: [i64; STRANDS],
    dc: [i64; STRANDS],
    born_l1: i64,
    contrast_defect: i64,
    backaction: i64,
    norm_err: i64,
    base_force: [i64; STRANDS],
    base_active: bool,

    mech_energy: i64,
    mech_flux_l1: i64,
    antisym_err: i64,
    mech_tension: [i64; STRANDS],
    mech_vel: [i64; STRANDS],
    mech_diss: [i64; STRANDS],
    flux_div: [i64; STRANDS],

    thermal_sigma: i64,
    thermal_excess: i64,
    min_temp: i64,
    min_entropy_prod: i64,
    bath_temp: [i64; STRANDS],

    info_kl: i64,
    info_surprise_mean: i64,
    info_surprise: [i64; STRANDS],

    chem_b_total: i64,
    chem_c_total: i64,
    chem_b: [i64; STRANDS],
    chem_mass_err: i64,
    min_species: i64,
    chem_rxn_heat: [i64; STRANDS],

    cog_fe_mean: i64,
    cog_fe: [i64; STRANDS],

    mid_delta: [i64; STRANDS],
    x1_spread: i64,
    x2_spread: i64,
}

#[derive(Clone, Debug)]
pub struct WebEngine {
    config: StackConfig,
    walk: CoinedWebWalk,
    interferometer: PhaseInterferometer,
    classical_walk: ClassicalWalkBase,
    mech: HarmonicWeb,
    flux: MomentumFlux,
    thermal: ThermalBath,
    info: InfoChannel,
    chem: ReactionKinetics,
    cog: PredictiveCognition,
    x1: LogisticChaos,
    x2: StandardMapChaos,
    prev_damp_mod: [i64; STRANDS],
    prev_rxn_heat: [i64; STRANDS],
    prev_mid_delta: [i64; STRANDS],
    prev_late_delta: [i64; STRANDS],
}

impl WebEngine {
    pub const fn new(config: StackConfig) -> Self {
        Self {
            config,
            walk: CoinedWebWalk::new(),
            interferometer: PhaseInterferometer,
            classical_walk: ClassicalWalkBase::new(),
            mech: HarmonicWeb::new(),
            flux: MomentumFlux,
            thermal: ThermalBath::new(),
            info: InfoChannel::new(),
            chem: ReactionKinetics::new(),
            cog: PredictiveCognition::new(),
            x1: LogisticChaos::new(),
            x2: StandardMapChaos::new(),
            prev_damp_mod: [0; STRANDS],
            prev_rxn_heat: [0; STRANDS],
            prev_mid_delta: [0; STRANDS],
            prev_late_delta: [0; STRANDS],
        }
    }

    pub const fn config(&self) -> &StackConfig {
        &self.config
    }

    fn init_scratch(&self, impulses: &[BusImpulse]) -> StepScratch {
        let mut s = StepScratch {
            active_mask: 0,
            bins: [0; STRANDS],
            mag: [0; STRANDS],
            disp: [0; STRANDS],
            skew: [0; STRANDS],
            press: [0; STRANDS],
            early_chaos: [0; STRANDS],
            probs: [0; STRANDS],
            contrast: [SCALE; STRANDS],
            dc: [0; STRANDS],
            born_l1: 0,
            contrast_defect: 0,
            backaction: 0,
            norm_err: 0,
            base_force: [0; STRANDS],
            base_active: false,
            mech_energy: 0,
            mech_flux_l1: 0,
            antisym_err: 0,
            mech_tension: [0; STRANDS],
            mech_vel: [0; STRANDS],
            mech_diss: [0; STRANDS],
            flux_div: [0; STRANDS],
            thermal_sigma: 0,
            thermal_excess: 0,
            min_temp: T_AMBIENT,
            min_entropy_prod: 0,
            bath_temp: self.thermal.temp,
            info_kl: 0,
            info_surprise_mean: 0,
            info_surprise: [0; STRANDS],
            chem_b_total: 0,
            chem_c_total: 0,
            chem_b: self.chem.b,
            chem_mass_err: 0,
            min_species: SCALE,
            chem_rxn_heat: [0; STRANDS],
            cog_fe_mean: 0,
            cog_fe: [0; STRANDS],
            mid_delta: [0; STRANDS],
            x1_spread: 0,
            x2_spread: 0,
        };

        let limit = 16 * SCALE;
        let pi4 = 4 * PI;
        for imp in impulses {
            if !imp.active {
                continue;
            }
            let idx = (imp.strand as usize) & (STRANDS - 1);
            s.active_mask |= 1u16 << idx;
            s.bins[idx] = imp.bin & 7;
            s.mag[idx] = clamp(s.mag[idx] + imp.magnitude.max(0), 0, limit);
            s.disp[idx] = clamp(s.disp[idx].max(imp.dispersion.max(0)), 0, limit);
            s.skew[idx] = clamp(s.skew[idx] + imp.phase_skew, -pi4, pi4);
            s.press[idx] = clamp(s.press[idx] + imp.queue_pressure.max(0), 0, limit);
        }
        s
    }

    fn step_chaos_at(
        &mut self,
        pos: ChaosPos,
        x1_drive: &[i64; STRANDS],
        x2_drive: &[i64; STRANDS],
        x1_spread: &mut i64,
        x2_spread: &mut i64,
    ) -> [i64; STRANDS] {
        let mut combined = [0i64; STRANDS];
        if self.config.x1 == pos {
            let r = self.x1.step(x1_drive, self.config.connected);
            *x1_spread = r.spread;
            for v in 0..STRANDS {
                combined[v] += r.delta[v];
            }
        }
        if self.config.x2 == pos {
            let r = self.x2.step(x2_drive);
            *x2_spread = r.spread;
            for v in 0..STRANDS {
                combined[v] += r.delta[v];
            }
        }
        combined
    }

    fn spatial_shear(
        mag: &[i64; STRANDS],
        press: &[i64; STRANDS],
        connected: bool,
    ) -> [i64; STRANDS] {
        let mut act = [0i64; STRANDS];
        for v in 0..STRANDS {
            act[v] = mag[v] + press[v];
        }
        let mut shear = [0i64; STRANDS];
        for v in 0..STRANDS {
            shear[v] = if connected {
                let avg_nbr = (act[cw(v, true)] + act[ccw(v, true)] + act[chord(v, true)]) / 3;
                act[v] - avg_nbr
            } else {
                act[v] >> 1
            };
        }
        shear
    }

    fn stage_base(&mut self, s: &mut StepScratch) {
        let mut grad_t = [0i64; STRANDS];
        let mut excess_t = [0i64; STRANDS];
        let connected = self.config.connected;
        if self.config.physics {
            for v in 0..STRANDS {
                let avg_t = (self.thermal.temp[cw(v, connected)]
                    + self.thermal.temp[ccw(v, connected)]
                    + self.thermal.temp[chord(v, connected)])
                    / 3;
                grad_t[v] = self.thermal.temp[v] - avg_t;
                excess_t[v] = (self.thermal.temp[v] - T_AMBIENT).abs();
            }
        }

        let mut phi_spiral = [0i64; STRANDS];
        let mut phi_chord = [0i64; STRANDS];
        let mut phi_anchor = [0i64; STRANDS];
        let mut activity = [0i64; STRANDS];
        let mut theta_mix = [0i64; STRANDS];

        for v in 0..STRANDS {
            activity[v] = s.mag[v] + s.press[v];
            phi_spiral[v] = s.skew[v] + (s.mag[v] >> 1) + s.early_chaos[v];
            phi_chord[v] = (s.press[v] >> 1) + s.early_chaos[v];
            phi_anchor[v] = (s.disp[v] >> 1) + (excess_t[v] >> 2);
            theta_mix[v] = clamp(
                (grad_t[v] >> 1) + ((self.thermal.temp[v] - T_AMBIENT) >> 2) + (s.skew[v] >> 3),
                -(HALF_PI >> 1),
                HALF_PI >> 1,
            );
        }

        match self.config.base {
            BaseMode::QuantumFull => {
                let _ = self.walk.step(&phi_spiral, &phi_chord, &phi_anchor, connected);
                let ir = self.interferometer.apply(&mut self.walk, &theta_mix);
                s.probs = ir.probs;
                s.contrast = ir.contrast;
                s.dc = ir.chiral;
                s.born_l1 = ir.born_l1;
                s.contrast_defect = ir.contrast_defect;
                s.backaction = ir.chiral_total;
                s.norm_err = ir.norm_err;
                s.base_active = true;
                for v in 0..STRANDS {
                    s.base_force[v] = ((ir.probs[v] - UNIFORM_PROB) << 4)
                        + ((SCALE - ir.contrast[v]) >> 1)
                        + (ir.chiral[v] << 2);
                }
            }
            BaseMode::QuantumNoInterferometer => {
                let qr = self.walk.step(&phi_spiral, &phi_chord, &phi_anchor, connected);
                s.probs = qr.probs;
                s.contrast = qr.contrast;
                s.born_l1 = qr.born_l1;
                s.contrast_defect = qr.contrast_defect;
                s.norm_err = qr.norm_err;
                s.base_active = true;
                for v in 0..STRANDS {
                    s.base_force[v] = ((qr.probs[v] - UNIFORM_PROB) << 4) + ((SCALE - qr.contrast[v]) >> 1);
                }
            }
            BaseMode::ClassicalWalkReplacement => {
                let cr = self.classical_walk.step(&activity, connected);
                s.probs = cr.probs;
                s.contrast = cr.contrast;
                s.born_l1 = cr.born_l1;
                s.norm_err = cr.norm_err;
                s.base_active = true;
                for v in 0..STRANDS {
                    s.base_force[v] = (cr.probs[v] - UNIFORM_PROB) << 4;
                }
            }
            BaseMode::NoBase => {
                s.probs = s.mag;
                s.base_active = false;
            }
        }
    }

    fn stage_mechanics(&mut self, s: &mut StepScratch, info_mod: &[i64; STRANDS]) {
        if !self.config.mechanics {
            return;
        }
        let shear = Self::spatial_shear(&s.mag, &s.press, self.config.connected);
        let mut drive = [0i64; STRANDS];
        for v in 0..STRANDS {
            let base_f = if s.base_active {
                s.base_force[v]
            } else {
                s.early_chaos[v]
            };
            drive[v] = shear[v] + (s.press[v] >> 1) + base_f + info_mod[v] + self.prev_mid_delta[v];
        }
        let mr = self.mech.step(&drive, &self.prev_damp_mod, self.config.connected);
        let fr = self.flux.measure(&self.mech, self.config.connected);
        s.mech_energy = mr.total_energy;
        s.mech_tension = mr.tension;
        s.mech_vel = mr.v;
        s.mech_diss = mr.dissipated;
        s.mech_flux_l1 = fr.flux_l1;
        s.antisym_err = fr.antisym_err;
        s.flux_div = fr.div_j;
    }

    fn stage_mid_chaos(&mut self, s: &mut StepScratch) {
        let mut d1 = [0i64; STRANDS];
        let mut d2 = [0i64; STRANDS];
        for v in 0..STRANDS {
            d1[v] = if self.config.mechanics {
                s.mech_tension[v] + (s.mag[v] >> 2)
            } else {
                s.mag[v] + s.press[v]
            };
            d2[v] = if self.config.mechanics && self.config.connected {
                s.flux_div[v].abs() + (s.press[v] >> 1)
            } else {
                s.press[v] + (s.disp[v] >> 1)
            };
        }
        let mut x1_sp = s.x1_spread;
        let mut x2_sp = s.x2_spread;
        s.mid_delta = self.step_chaos_at(ChaosPos::Mid, &d1, &d2, &mut x1_sp, &mut x2_sp);
        s.x1_spread = x1_sp;
        s.x2_spread = x2_sp;
        self.prev_mid_delta = s.mid_delta;
    }

    fn stage_info(&mut self, s: &mut StepScratch) {
        if !self.config.info {
            return;
        }
        let ir = self.info.step(s.active_mask, &s.bins, &s.disp, &s.probs);
        s.info_kl = ir.kl_div;
        s.info_surprise_mean = ir.surprise_mean;
        s.info_surprise = ir.surprise;
    }

    fn stage_physics(&mut self, s: &mut StepScratch, extra_heat: &[i64; STRANDS]) {
        if !self.config.physics {
            return;
        }
        let mut heat = [0i64; STRANDS];
        for v in 0..STRANDS {
            heat[v] = s.mech_diss[v] + extra_heat[v];
        }
        let tr = self.thermal.step(&heat, self.config.connected);
        s.thermal_sigma = tr.entropy_prod_total;
        s.thermal_excess = tr.excess_temp_total;
        s.min_temp = tr.min_temp;
        s.min_entropy_prod = tr.min_entropy_prod;
        s.bath_temp = tr.temp;
    }

    fn stage_chemistry(&mut self, s: &mut StepScratch, extra_drive: &[i64; STRANDS]) {
        if !self.config.chemistry {
            return;
        }
        let mut drive_s = [0i64; STRANDS];
        for v in 0..STRANDS {
            drive_s[v] = s.info_surprise[v] + extra_drive[v] + s.mid_delta[v].abs();
        }
        let cr = self.chem.step(&s.bath_temp, &drive_s, self.config.connected);
        s.chem_b_total = cr.b_total;
        s.chem_c_total = cr.c_total;
        s.chem_b = cr.b;
        s.chem_mass_err = cr.max_mass_err;
        s.min_species = cr.min_species;
        s.chem_rxn_heat = cr.rxn_heat;
        self.prev_rxn_heat = cr.rxn_heat;
    }

    fn stage_cognition(&mut self, s: &mut StepScratch, include_chem: bool) {
        let mut obs_y = [0i64; STRANDS];
        for v in 0..STRANDS {
            let base_y = if s.base_active {
                ((s.probs[v] - UNIFORM_PROB).abs() << 3) + ((SCALE - s.contrast[v]) >> 2)
            } else {
                s.mag[v]
            };
            let mech_y = if self.config.mechanics {
                s.mech_tension[v] >> 1
            } else {
                0
            };
            let info_y = if self.config.info {
                s.info_surprise[v] >> 1
            } else {
                0
            };
            let chem_y = if include_chem && self.config.chemistry {
                (s.chem_b[v] - (SCALE / 10)).abs() << 2
            } else {
                0
            };
            obs_y[v] = base_y + mech_y + info_y + chem_y + self.prev_late_delta[v];
        }

        if self.config.cognition {
            let gr = self.cog.step(&obs_y, self.config.connected);
            s.cog_fe = gr.fe;
            s.cog_fe_mean = gr.free_energy_mean;
            self.prev_damp_mod = gr.damp_mod;
        } else {
            self.prev_damp_mod = [0; STRANDS];
        }

        let mut late_d2 = [0i64; STRANDS];
        for v in 0..STRANDS {
            late_d2[v] = s.cog_fe[v] + s.info_surprise[v];
        }
        let mut x1_sp = s.x1_spread;
        let mut x2_sp = s.x2_spread;
        let late_delta = self.step_chaos_at(ChaosPos::Late, &obs_y, &late_d2, &mut x1_sp, &mut x2_sp);
        s.x1_spread = x1_sp;
        s.x2_spread = x2_sp;
        self.prev_late_delta = late_delta;
    }

    pub fn step(&mut self, impulses: &[BusImpulse]) -> WebObservation {
        let mut s = self.init_scratch(impulses);

        let shear = Self::spatial_shear(&s.mag, &s.press, self.config.connected);
        let mut early_x1 = [0i64; STRANDS];
        let mut early_x2 = [0i64; STRANDS];
        for v in 0..STRANDS {
            early_x1[v] = shear[v].abs() + s.disp[v];
            early_x2[v] = s.skew[v].abs() + (s.press[v] >> 1);
        }
        let mut x1_sp = 0i64;
        let mut x2_sp = 0i64;
        s.early_chaos = self.step_chaos_at(ChaosPos::Early, &early_x1, &early_x2, &mut x1_sp, &mut x2_sp);
        s.x1_spread = x1_sp;
        s.x2_spread = x2_sp;

        self.stage_base(&mut s);

        let zero = [0i64; STRANDS];
        match self.config.order {
            StackOrder::OrderA => {
                self.stage_mechanics(&mut s, &zero);
                self.stage_mid_chaos(&mut s);
                let rxn_prev = self.prev_rxn_heat;
                self.stage_physics(&mut s, &rxn_prev);
                self.stage_info(&mut s);
                self.stage_chemistry(&mut s, &zero);
                self.stage_cognition(&mut s, true);
            }
            StackOrder::OrderB => {
                self.stage_info(&mut s);
                let mut surp_drive = [0i64; STRANDS];
                for v in 0..STRANDS {
                    surp_drive[v] = s.info_surprise[v] >> 1;
                }
                self.stage_mechanics(&mut s, &surp_drive);
                self.stage_mid_chaos(&mut s);
                let mut mech_extra = [0i64; STRANDS];
                for v in 0..STRANDS {
                    mech_extra[v] = s.mech_tension[v] >> 1;
                }
                self.stage_chemistry(&mut s, &mech_extra);
                let rxn_now = s.chem_rxn_heat;
                self.stage_physics(&mut s, &rxn_now);
                self.stage_cognition(&mut s, true);
            }
            StackOrder::OrderC => {
                self.stage_mechanics(&mut s, &zero);
                self.stage_mid_chaos(&mut s);
                self.stage_info(&mut s);
                self.stage_cognition(&mut s, false);
                let mut cog_extra = [0i64; STRANDS];
                for v in 0..STRANDS {
                    cog_extra[v] = s.cog_fe[v] >> 1;
                }
                self.stage_chemistry(&mut s, &cog_extra);
                let rxn_now = s.chem_rxn_heat;
                self.stage_physics(&mut s, &rxn_now);
            }
        }

        let mut strand_response = [0i64; STRANDS];
        for v in 0..STRANDS {
            let mut resp = 0i64;
            if s.base_active {
                resp += ((s.probs[v] - UNIFORM_PROB).abs() << 4) + ((SCALE - s.contrast[v]) >> 1);
                if self.config.base == BaseMode::QuantumFull {
                    resp += s.dc[v] << 2;
                }
            }
            if self.config.mechanics {
                resp += s.mech_tension[v] + (s.mech_vel[v].abs() >> 1);
            }
            if self.config.physics {
                resp += (s.bath_temp[v] - T_AMBIENT).abs() << 1;
            }
            if self.config.info {
                resp += s.info_surprise[v];
            }
            if self.config.chemistry {
                resp += (s.chem_b[v] - (SCALE / 10)).abs() << 2;
            }
            if self.config.cognition {
                resp += s.cog_fe[v];
            }
            strand_response[v] = resp;
        }

        WebObservation {
            scalars: [
                s.contrast_defect,
                s.born_l1,
                s.backaction,
                s.mech_energy,
                s.mech_flux_l1,
                s.thermal_sigma,
                s.thermal_excess,
                s.info_kl,
                s.info_surprise_mean,
                s.chem_b_total,
                s.chem_c_total,
                s.cog_fe_mean,
            ],
            strand_response,
            active_obs_mask: self.config.active_obs_mask(),
            unitary_norm_err: s.norm_err,
            chem_mass_err: s.chem_mass_err,
            antisym_err: s.antisym_err,
            min_temp: s.min_temp,
            min_entropy_prod: s.min_entropy_prod,
            min_species: s.min_species,
            x1_spread: s.x1_spread,
            x2_spread: s.x2_spread,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_web_fp::QUARTER;

    fn sample_impulses(tick: usize) -> [BusImpulse; 2] {
        [
            BusImpulse {
                strand: 3,
                bin: (tick % 3) as u8,
                magnitude: SCALE >> 1,
                dispersion: QUARTER,
                phase_skew: if tick % 2 == 0 { QUARTER } else { -QUARTER },
                queue_pressure: QUARTER,
                active: true,
            },
            BusImpulse {
                strand: 7,
                bin: (tick % 5) as u8,
                magnitude: SCALE,
                dispersion: SCALE >> 1,
                phase_skew: SCALE >> 2,
                queue_pressure: SCALE >> 1,
                active: true,
            },
        ]
    }

    #[test]
    fn deterministic_and_invariant_safe_across_all_orders() {
        for order in [StackOrder::OrderA, StackOrder::OrderB, StackOrder::OrderC] {
            let mut e1 = WebEngine::new(StackConfig::full(order).with_chaos(ChaosPos::Early, ChaosPos::Late));
            let mut e2 = WebEngine::new(StackConfig::full(order).with_chaos(ChaosPos::Early, ChaosPos::Late));
            for t in 0..300 {
                let imps = sample_impulses(t);
                let o1 = e1.step(&imps);
                let o2 = e2.step(&imps);
                assert_eq!(o1, o2);
                assert!(o1.unitary_norm_err <= SCALE / 10_000);
                assert!(o1.chem_mass_err <= SCALE / 10_000);
                assert_eq!(o1.antisym_err, 0);
                assert!(o1.min_temp >= atom_web_fp::T_MIN);
                assert!(o1.min_entropy_prod >= 0);
                assert!(o1.min_species >= 0);
            }
        }
    }

    #[test]
    fn order_quantum_and_connectivity_sensitivity() {
        let mut ea = WebEngine::new(StackConfig::full(StackOrder::OrderA));
        let mut eb = WebEngine::new(StackConfig::full(StackOrder::OrderB));
        let mut ec = WebEngine::new(StackConfig::full(StackOrder::OrderC));
        let mut cr = WebEngine::new(StackConfig {
            base: BaseMode::ClassicalWalkReplacement,
            ..StackConfig::full(StackOrder::OrderA)
        });
        let mut dc = WebEngine::new(StackConfig {
            connected: false,
            ..StackConfig::full(StackOrder::OrderA)
        });

        let mut oa = None;
        let mut ob = None;
        let mut oc = None;
        let mut ocr = None;
        let mut odc = None;
        for t in 0..40 {
            let imps = sample_impulses(t);
            oa = Some(ea.step(&imps));
            ob = Some(eb.step(&imps));
            oc = Some(ec.step(&imps));
            ocr = Some(cr.step(&imps));
            odc = Some(dc.step(&imps));
        }
        let (oa, ob, oc, ocr, odc) = (oa.unwrap(), ob.unwrap(), oc.unwrap(), ocr.unwrap(), odc.unwrap());
        assert_ne!(oa.scalars, ob.scalars);
        assert_ne!(ob.scalars, oc.scalars);
        assert_ne!(oa.scalars, oc.scalars);
        assert_ne!(oa.scalars[1], ocr.scalars[1]);
        assert_ne!(oa.strand_response, odc.strand_response);
    }
}
