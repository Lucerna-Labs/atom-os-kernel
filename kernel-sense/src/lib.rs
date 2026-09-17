//! kernel-sense — E21, the shadow web over the kernel.
//!
//! The kernel is message-shaped: every inter-task message and every
//! state mutation flows through the syscall dispatcher (the bus's
//! ground level, embryo verified at kernel-orchestrator syscall.rs).
//! This crate turns that chokepoint into a sensor:
//!
//! - **Vibrations**: feature-rate records (pid, syscall, bytes,
//!   target) collected at dispatch — never payloads, only behavior.
//! - **Scars**: a small substrate field (128 sites). Activity raises
//!   permeability where it lands; erosion relaxes it unconditionally.
//!   Site = mix(pid, target) — the field learns WHO TALKS TO WHOM as
//!   material state. Quiet misuse accumulates a budget; quiet time
//!   erases it (thermodynamic forgetting).
//! - **The cone**: a normal site map trained from a clean-boot
//!   learning phase (E20's lightcone discipline: train on normality,
//!   admit what fits, reject the foreign). After freeze, events that
//!   scar sites outside the map spend foreign budget for their pid.
//! - **The spider**: `quarantined(pid)` — the scheduler asks one
//!   question and simply stops scheduling tasks the cone has
//!   condemned. A task that never runs cannot act: the veil dial,
//!   expressed as policy.
//!
//! no_std + core only (f32 via hardware SSE in task context). State
//! lives behind a tiny spin lock touched only from syscall/scheduler
//! context — the same reentrancy envelope as the mailbox code it
//! sits beside.

#![cfg_attr(not(feature = "std"), no_std)]

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// Field geometry: 16x8 = 128 conversation sites.
pub const WIDTH: usize = 16;
pub const HEIGHT: usize = 8;
pub const SITES: usize = WIDTH * HEIGHT;
/// Learning-phase capacity: events recorded before auto-freeze.
pub const HISTORY: usize = 4096;
/// Permeability law (ported from the substrate, E20 calibration).
pub const FLOOR: f32 = 0.05;
pub const READABLE: f32 = 0.14;
const FORMATION: f32 = 0.03;
const EROSION: f32 = 0.0018;
/// Erosion decouples from event rate: one erosion pass per 16 events,
/// so a site's equilibrium reflects its ABSOLUTE activity, not its
/// share of total traffic. Without this, spreading traffic across N
/// sites caps every site's scar depth at ~1/N of the single-site
/// equilibrium — wide-but-normal traffic could never scar.
const EROSION_INTERVAL: u64 = 16;
const ENERGY_PER_UNIT: f32 = 0.02;
/// Foreign budget above which a pid is condemned. Calibrated so a
/// clean boot's stray events never reach it (T2 judgment; T3
/// measurement pending the QEMU gate).
pub const QUARANTINE_BUDGET: f32 = 2.0;
const MAX_TRACKED: usize = 16;

/// Minimal spin lock (no dependencies; kernel context only).
pub struct Lock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Lock<T> {}

pub struct Guard<'a, T> {
    lock: &'a Lock<T>,
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Lock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
        Guard { lock: self }
    }
}

struct Sensor {
    permeability: [f32; SITES],
    foreign: [(u64, f32); MAX_TRACKED],
    trained: bool,
    normal_map: [bool; SITES],
    events: u64,
}

static SENSOR: Lock<Sensor> = Lock::new(Sensor {
    permeability: [FLOOR; SITES],
    foreign: [(0, 0.0); MAX_TRACKED],
    trained: false,
    normal_map: [false; SITES],
    events: 0,
});

/// Deterministic site for a (pid, target, syscall) conversation.
pub fn site_for(pid: u64, target: u64, syscall: u64) -> usize {
    let mut z = pid ^ (target << 17) ^ syscall.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((z ^ (z >> 31)) % SITES as u64) as usize
}

/// Record one syscall vibration at the dispatch chokepoint. Cheap by
/// law: a hash, two adds, and the material tick — erosion runs over
/// the whole field every event (the substrate law decays everywhere,
/// not only where events land), which is 128 f32 steps.
pub fn record(pid: u64, syscall: u64, target: u64, weight: f32) {
    let site = site_for(pid, target, syscall);
    let mut sensor = SENSOR.lock();
    let energy = weight * ENERGY_PER_UNIT;
    let growth = FORMATION * energy.min(1.0) * (1.0 - sensor.permeability[site]);
    sensor.permeability[site] = (sensor.permeability[site] + growth).clamp(FLOOR, 1.0);
    let events = sensor.events + 1;
    sensor.events = events;
    // Global erosion tick, rate-decoupled: quiet erodes every site
    // unconditionally, but on wall-activity cadence, not per event.
    if events % EROSION_INTERVAL == 0 {
        for p in sensor.permeability.iter_mut() {
            let decay = EROSION * (*p - FLOOR).max(0.0);
            *p = (*p - decay).max(FLOOR);
        }
    }

    if !sensor.trained {
        if events >= HISTORY as u64 {
            let map = learned_map(&sensor.permeability);
            if let Some(map) = map {
                sensor.normal_map = map;
                sensor.trained = true;
            }
        }
        return;
    }
    if !sensor.normal_map[site] {
        let weight_now = energy;
        let mut claimed = false;
        for entry in sensor.foreign.iter_mut() {
            if entry.0 == pid {
                entry.1 += weight_now;
                claimed = true;
                break;
            }
        }
        if !claimed {
            if let Some(entry) = sensor.foreign.iter_mut().find(|e| e.0 == 0) {
                *entry = (pid, weight_now);
            }
            // Tracking table full: the loudest already-tracked pids
            // carry the verdict; dropping a newcomer is the honest
            // degradation (better than evicting evidence).
        }
    }
}

/// The normal map: every site scarred to READABLE depth by the
/// learning phase (sustained conversation, ~900 events — a single
/// stray event must not widen the map), dilated one Moore ring.
/// None if nothing raised yet — the sensor stays learning rather
/// than freezing a map that condemns everything.
fn learned_map(permeability: &[f32; SITES]) -> Option<[bool; SITES]> {
    let mut raised = [false; SITES];
    let mut any = false;
    for (site, &p) in permeability.iter().enumerate() {
        if p > READABLE {
            raised[site] = true;
            any = true;
        }
    }
    if !any {
        return None;
    }
    let mut map = raised;
    for site in 0..SITES {
        if !raised[site] {
            continue;
        }
        let x = site % WIDTH;
        let y = site / WIDTH;
        for dy in [0isize, 1, -1] {
            for dx in [0isize, 1, -1] {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if (0..WIDTH as isize).contains(&nx) && (0..HEIGHT as isize).contains(&ny) {
                    map[(ny as usize) * WIDTH + nx as usize] = true;
                }
            }
        }
    }
    Some(map)
}

/// Force-freeze now (the SYS_SENSE control path).
pub fn freeze() {
    let mut sensor = SENSOR.lock();
    if let Some(map) = learned_map(&sensor.permeability) {
        sensor.normal_map = map;
        sensor.trained = true;
    }
}

/// Foreign budget a pid has accumulated outside the normal map.
pub fn foreign_budget(pid: u64) -> f32 {
    let sensor = SENSOR.lock();
    for &(tracked, budget) in sensor.foreign.iter() {
        if tracked == pid {
            return budget;
        }
    }
    0.0
}

/// The spider's one question: is this pid condemned? A quarantined
/// task is never scheduled — the veil dial as scheduler policy.
pub fn quarantined(pid: u64) -> bool {
    let sensor = SENSOR.lock();
    sensor.trained
        && sensor
            .foreign
            .iter()
            .any(|&(tracked, budget)| tracked == pid && budget > QUARANTINE_BUDGET)
}

/// Compact status: (trained, events, raised sites, readable sites).
pub fn status() -> (bool, u64, usize, usize) {
    let sensor = SENSOR.lock();
    let raised = sensor
        .permeability
        .iter()
        .filter(|&&p| p > FLOOR + 0.005)
        .count();
    let readable = sensor.permeability.iter().filter(|&&p| p > READABLE).count();
    (sensor.trained, sensor.events, raised, readable)
}

/// Learning still open?
pub fn learning() -> bool {
    !SENSOR.lock().trained
}

/// Reset (host tests only; the kernel never resets its web).
pub fn reset() {
    let mut sensor = SENSOR.lock();
    sensor.permeability = [FLOOR; SITES];
    sensor.foreign = [(0, 0.0); MAX_TRACKED];
    sensor.trained = false;
    sensor.normal_map = [false; SITES];
    sensor.events = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean boot's conversation pattern: a few pids talking to
    /// their usual partners through their usual syscalls.
    fn clean_boot(events: usize) {
        for i in 0..events {
            let pid = (i % 3) as u64 + 1;
            let target = ((i / 3) % 2) as u64 + 1;
            let syscall = if i % 2 == 0 { 15 } else { 5 };
            record(pid, syscall, target, 1.0);
        }
    }

    #[test]
    fn gate_s1_learning_then_freeze_then_clean_admits() {
        reset();
        clean_boot(2000);
        freeze();
        assert!(learning() == false);
        let (trained, events, raised, _readable) = status();
        assert!(trained && events == 600 && raised > 0);
        // Post-freeze, the SAME clean conversations accumulate no
        // foreign budget for anyone.
        clean_boot(4000);
        for pid in 1..=3 {
            assert_eq!(foreign_budget(pid), 0.0, "clean pid {pid} flagged");
            assert!(!quarantined(pid));
        }
    }

    #[test]
    fn gate_s2_rogue_conversation_is_condemned() {
        reset();
        clean_boot(2000);
        freeze();
        // A rogue pid talking to a partner no clean boot ever used —
        // every event lands foreign and the budget accumulates.
        for _ in 0..400 {
            record(9, 15, 7, 1.0);
        }
        assert!(
            foreign_budget(9) > QUARANTINE_BUDGET,
            "rogue budget {} must exceed {}",
            foreign_budget(9),
            QUARANTINE_BUDGET
        );
        assert!(quarantined(9));
    }

    #[test]
    fn gate_s3_budget_not_threshold_one_stranger_event_is_not_doom() {
        reset();
        clean_boot(2000);
        freeze();
        // A single foreign event (benign novelty) does not condemn —
        // detection is a budget, not a per-event threshold.
        record(4, 15, 9, 1.0);
        assert!(!quarantined(4));
    }

    #[test]
    fn gate_s4_quiet_scars_erode() {
        reset();
        for _ in 0..2000 {
            record(1, 15, 2, 1.0);
        }
        let (_t, _e, raised_before, _r) = status();
        assert!(raised_before > 0);
        // Long quiet: erosion relaxes everything toward the floor.
        for _ in 0..300_000 {
            record(0, 0, 0, 0.0);
        }
        let (_t2, _e2, raised_after, _r2) = status();
        assert_eq!(raised_after, 0, "unmaintained scars must erode");
    }
}
