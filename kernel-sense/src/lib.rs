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
/// Budget decay per scheduler tick (linear). Condemnation is
/// thermodynamic, not permanent: a starved pid's budget erodes with
/// wall time, so a false positive earns its scheduling back, while a
/// real rogue re-charges the moment it resumes and is re-condemned —
/// the veil dial breathes. Full condemnation budget (say 40) clears
/// in ~3000 ticks.
pub const BUDGET_DECAY_PER_TICK: f32 = 0.013;
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

/// Rhythm window per tracked stream (E30's organ, kernel edition).
const GAP_WINDOW: usize = 48;
const RHYTHM_SLOTS: usize = 24; // boot runs ~10 payloads; the organ must track them all
/// Current PE below baseline - DRIFT_MARGIN and under DRIFT_CEILING
/// = machine-ward drift: the takeover signal.
const DRIFT_MARGIN: f64 = 0.12;
const DRIFT_CEILING: f64 = 0.65;

/// One stream's rhythm: inter-event gaps (ticks), ring-buffered,
/// baseline matured when the window first fills (per-pid — a new
/// stream EARNS its baseline as it runs; drift is judged only
/// against matured baselines). A drift verdict is latched for the
/// stream's life: takeover is a one-way observation (reboot is the
/// ceremony).
struct RhythmSlot {
    pid: u64,
    last_tick: u64,
    gaps: [u64; GAP_WINDOW],
    len: usize,
    head: usize,
    baseline: f64,
    matured: bool,
    drifted: bool,
}

impl RhythmSlot {
    const EMPTY: Self = Self {
        pid: u64::MAX,
        last_tick: u64::MAX,
        gaps: [0; GAP_WINDOW],
        len: 0,
        head: 0,
        baseline: 0.0,
        matured: false,
        drifted: false,
    };

    fn push_gap(&mut self, gap: u64) {
        self.gaps[self.head] = gap;
        self.head = (self.head + 1) % GAP_WINDOW;
        if self.len < GAP_WINDOW {
            self.len += 1;
        }
    }

}

/// log2 in-repo (f64::log2 is std-only) — mirrors kernel-egress's
/// implementation (exponent from bits + atanh series).
fn log2(x: f64) -> f64 {
    const LN2: f64 = 0.693_147_180_559_945_3;
    if x <= 0.0 {
        return 0.0;
    }
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7FF) as i64 - 1023;
    let mantissa = 1.0 + (bits & 0xF_FFFF_FFFF_FFFF) as f64 / 4_503_599_627_370_496.0;
    let z = (mantissa - 1.0) / (mantissa + 1.0);
    let z2 = z * z;
    let series = 2.0 * (z + z * z2 / 3.0 + z * z2 * z2 / 5.0) / LN2;
    exponent as f64 + series
}

/// Position-stable ordinal pattern (atom-writer tie rule).
fn ordinal_pattern(a: u64, b: u64, c: u64) -> usize {
    let mut sorted = [(a, 0usize), (b, 1), (c, 2)];
    sorted.sort_unstable_by_key(|&(value, position)| (value, position));
    let mut ranks = [0usize; 3];
    for (rank, &(_, position)) in sorted.iter().enumerate() {
        ranks[position] = rank;
    }
    ranks[0] * 2 + usize::from(ranks[1] > ranks[2])
}

/// Bandt-Pompe PE over the ring window, iterating chronologically.
fn ring_pe(slot: &RhythmSlot) -> f64 {
    if slot.len < 16 {
        return -1.0; // too few gaps: no verdict
    }
    let mut counts = [0usize; 6];
    // Iterate consecutive triples in ring order.
    let at = |index: usize| -> u64 { slot.gaps[(slot.head + slot.len + index - GAP_WINDOW) % GAP_WINDOW] };
    // Simpler chronological walk: oldest is (head - len) mod WINDOW.
    let oldest = (slot.head + GAP_WINDOW - slot.len) % GAP_WINDOW;
    for step in 0..slot.len.saturating_sub(2) {
        let i0 = (oldest + step) % GAP_WINDOW;
        let i1 = (oldest + step + 1) % GAP_WINDOW;
        let i2 = (oldest + step + 2) % GAP_WINDOW;
        counts[ordinal_pattern(slot.gaps[i0], slot.gaps[i1], slot.gaps[i2])] += 1;
    }
    let total = (slot.len - 2) as f64;
    let mut bits = 0.0;
    for count in counts {
        if count > 0 {
            let p = count as f64 / total;
            bits -= p * log2(p);
        }
    }
    bits / log2(6.0)
}

struct Sensor {
    permeability: [f32; SITES],
    foreign: [(u64, f32); MAX_TRACKED],
    trained: bool,
    normal_map: [bool; SITES],
    events: u64,
    /// Scheduler ticks (advanced by tick(); the rhythm's clock).
    ticks: u64,
    rhythm: [RhythmSlot; RHYTHM_SLOTS],
}

static SENSOR: Lock<Sensor> = Lock::new(Sensor {
    permeability: [FLOOR; SITES],
    foreign: [(0, 0.0); MAX_TRACKED],
    trained: false,
    normal_map: [false; SITES],
    events: 0,
    ticks: 0,
    rhythm: [RhythmSlot::EMPTY; RHYTHM_SLOTS],
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

    // E31 rhythm tracking: the gap between this pid's successive
    // events feeds its ring window; baseline matures per-pid once the
    // window fills (a stream EARNS its baseline as it runs); drift
    // (machine-ward, from a matured organic baseline) is a latched,
    // one-way verdict that fires the intrusion signal.
    {
        let ticks_now = sensor.ticks;
        let mut slot_pid = sensor
            .rhythm
            .iter()
            .position(|slot| slot.pid == pid);
        if slot_pid.is_none() {
            if let Some(free) = sensor.rhythm.iter().position(|slot| slot.pid == u64::MAX) {
                sensor.rhythm[free] = RhythmSlot { pid, ..RhythmSlot::EMPTY };
                slot_pid = Some(free);
            }
        }
        if let Some(index) = slot_pid {
            let trained_now = sensor.trained;
            let slot = &mut sensor.rhythm[index];
            if slot.last_tick != u64::MAX {
                let gap = ticks_now.wrapping_sub(slot.last_tick);
                slot.push_gap(gap);
            }
            slot.last_tick = ticks_now;
            if trained_now && slot.len == GAP_WINDOW {
                let current = ring_pe(slot);
                if !slot.matured {
                    slot.baseline = current;
                    slot.matured = true;
                } else if !slot.drifted
                    && slot.baseline - current > DRIFT_MARGIN
                    && current < DRIFT_CEILING
                {
                    slot.drifted = true;
                    let condemned = slot.pid;
                    let _ = &condemned;
                    // Fire through the atomic ring outside the lock:
                    // drop, fire, return (this event IS the verdict).
                    drop(sensor);
                    fire_intrusion_signal(pid);
                    return;
                }
            }
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
    let tripped = {
        let sensor = SENSOR.lock();
        sensor.trained
            && sensor
                .foreign
                .iter()
                .any(|&(tracked, budget)| tracked == pid && budget > QUARANTINE_BUDGET)
    };
    if tripped {
        fire_intrusion_signal(pid);
    }
    tripped
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

/// Scheduler-tick maintenance: erodes foreign budgets with wall
/// time. Called from timer context; the permeability field stays
/// event-driven (scars record activity, budgets record blame).
pub fn tick() {
    let mut sensor = SENSOR.lock();
    sensor.ticks = sensor.ticks.wrapping_add(1);
    for entry in sensor.foreign.iter_mut() {
        if entry.1 > 0.0 {
            entry.1 = (entry.1 - BUDGET_DECAY_PER_TICK).max(0.0);
        }
    }
}

// E22: the spider's signal wire. When quarantined() trips for a pid
// not yet reported, the pid is queued exactly once for the shadow
// web's listeners (the fail-dead key, first among them).
static INTRUSION_QUEUE: [core::sync::atomic::AtomicU64; 8] = [
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
    core::sync::atomic::AtomicU64::new(u64::MAX),
];
static INTRUSION_HEAD: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn fire_intrusion_signal(pid: u64) {
    use core::sync::atomic::Ordering;
    let head = INTRUSION_HEAD.fetch_add(1, Ordering::AcqRel) as usize;
    INTRUSION_QUEUE[head % 8].store(pid, Ordering::Release);
}

/// Take one condemned pid (oldest first), if any. The shadow web's
/// destruction listeners poll this from scheduler context.
pub fn take_intrusion_signal() -> Option<u64> {
    use core::sync::atomic::Ordering;
    for slot in INTRUSION_QUEUE.iter() {
        let pid = slot.swap(u64::MAX, Ordering::AcqRel);
        if pid != u64::MAX {
            return Some(pid);
        }
    }
    None
}

/// Rhythm status for a pid: (matured, drifted, baseline x1024,
/// current x1024). Current is -1 (encoded 0) when the window is
/// still short.
pub fn rhythm_status(pid: u64) -> (bool, bool, u64, u64) {
    let sensor = SENSOR.lock();
    match sensor.rhythm.iter().find(|slot| slot.pid == pid) {
        Some(slot) => {
            let current = if slot.len >= 16 { ring_pe(slot) } else { -1.0 };
            (
                slot.matured,
                slot.drifted,
                (slot.baseline.max(0.0) * 1024.0) as u64,
                (current.max(0.0) * 1024.0) as u64,
            )
        }
        None => (false, false, 0, 0),
    }
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
    sensor.ticks = 0;
    sensor.rhythm = [RhythmSlot::EMPTY; RHYTHM_SLOTS];
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
    fn gate_s5_false_positive_recovers_thermodynamically() {
        reset();
        clean_boot(2000);
        freeze();
        // A benign pid wrongly condemned (foreign budget spent by a
        // misclassified burst) stops; wall time erodes the budget and
        // the scheduler takes it back.
        for _ in 0..500 {
            record(5, 15, 42, 1.0);
        }
        assert!(quarantined(5), "budget must condemn first");
        let mut ticks = 0;
        while quarantined(5) {
            tick();
            ticks += 1;
            assert!(ticks < 100_000, "release must terminate");
        }
        assert!(!quarantined(5));
        assert!(ticks > 0);
    }

    #[test]
    fn gate_s6_persistent_rogue_is_re_condemned_every_cycle() {
        reset();
        clean_boot(2000);
        freeze();
        let mut releases = 0;
        for _cycle in 0..3 {
            // Rogue misbehaves until condemned.
            let mut guard = 0;
            while !quarantined(9) {
                record(9, 15, 7, 1.0);
                guard += 1;
                assert!(guard < 100_000, "condemnation must arrive");
            }
            // Starved (no events possible); wall time erodes release.
            while quarantined(9) {
                tick();
            }
            releases += 1;
        }
        assert_eq!(releases, 3, "the dial must breathe: condemn, release, re-condemn");
        // And the rogue's next burst re-condemns immediately.
        let mut guard = 0;
        while !quarantined(9) {
            record(9, 15, 7, 1.0);
            guard += 1;
            assert!(guard < 100_000);
        }
        assert!(guard < 5000, "re-condemnation must be fast: {guard} events");
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
