# PRECOMMIT: Component 1 (Phase 1A-v2) — Unitary Interferometer & Spatial-Shear Composition Update

**Status:** FROZEN BEFORE PHASE 1A-v2 CODE EDITS OR EXECUTION  
**Scope:** Two-crate `#![no_std]` Atom stack (`atom-web-fp` + `atom-web-engine`) evaluated across the **exact unchanged** 41-arm × 7-regime × 3-seed (**861 runs**) test harness, writing to `experiments/c1-spiderweb/results-v2/`.  
**Prior Baseline Archived:** All v1 evidence remains preserved unmodified in `docs/PRECOMMIT-C1-SPIDERWEB.md` and `experiments/c1-spiderweb/results-v1/`.

---

## 1. Preserved Phase 1A-v1 Empirical Findings & Diagnosed Composition Defects

Phase 1A-v1 executed 861/861 runs cleanly (`63/63` native tests, zero Rule 1 keywords, `no_std` `--locked --offline` clean). Observed v1 measurements directly identified three first-principle coupling/composition defects (with no change to frozen metrics, scenarios, or invariants):

1. **v1 Reference Measurements (`results-v1/`):**
   - `Abl_NoWeakMeasurement`: `J = 0.7675`, `AUC_all = 0.8161`, `AUC_hard = 0.5880`, `WebProp = 0.7697`, `Recov = 0.9698`, `R2 = 0.793`, `R3 = 0.505`, `R4 = 0.291`, `R5 = 0.968`, `H1..H7 = ALL PASS` (`4326.1 ns/tick`).
   - `Baseline_NonAtom_Ewma`: `J = 0.6715`, `AUC_all = 0.8463`, `AUC_hard = 0.5959`, `WebProp = 0.0000`, `Recov = 0.9941`, `R2 = 0.983`, `R3 = 0.392`, `R4 = 0.399`, `R5 = 0.997` (`37.2 ns/tick`).
   - `OrderA_NoChaos`: `J = 0.5620`, `AUC_all = 0.6006`, `AUC_hard = 0.2128`, `WebProp = 0.8171`, `Recov = 0.8600`, `R3 = 0.007`, `R4 = 0.005`, `H1..H7 = ALL PASS`.
   - `Ctrl_ClassicalUpperOnly`: `J = 0.4632`, `WebProp = 0.0207`; `Ctrl_ReplacementBaseDiffusion`: `J = 0.4660`, `WebProp = 0.0000`; `Ctrl_BaseOnly`: `J = 0.5362`, `H5 = FAIL` (`rec_rat = 0.9441`).
   - Rule 5: `OrderA_X1Mid_X2Late` promoted slightly (`J = 0.5658`, `+0.0038` over `OrderA_NoChaos`, beating both `X1_Mid` `0.5640` and `X2_Late` `0.5627`), while `Early` `X1` regressed across all three orders (`-0.0121` to `-0.0664`).

2. **Three Exact Composition Defects Diagnosed from v1 Evidence:**
   - **Defect 1 — Q2 (`WeakMeasurement`) Amplitude-Collapse Defect:** Non-unitary anchor scaling `(1 + lambda_v)` driven by raw `activity = mag + press` concentrated probability mass onto high-amplitude `R1_benign_burst` strands every tick, extinguishing `CoinedWebWalk` phase interference (`R3` dropped `0.505 -> 0.007` and `OrderA_NoChaos` dropped `0.7675 -> 0.5620`).
   - **Defect 2 — I1 (`InfoChannel`) Firing-Rate Bias:** Decaying `surprise_state` toward `0` on inactive ticks caused dense habitual firing (`R1_benign_burst`) to hold a higher channel mean than sparser high-entropy `R4_entropy_shift`.
   - **Defect 3 — Raw Amplitude Drive on `Early` Chaos & `HarmonicWeb`, plus `X2` Kick Momentum:** Feeding raw scalar `mag` into `Early` `LogisticChaos` and `HarmonicWeb` amplified coherent multi-strand bursts (`R1`), while exporting raw `sin(theta_next - theta)` from symplectic `StandardMapChaos` retained post-kick phase drift during `R6_recovery`.

---

## 2. Frozen v2 Mechanical & Composition Updates (Rule 1 Compliant, Unlabeled)

All **7 regimes, 3 seeds, Hard Invariants H1–H7, frozen Benefit Metric `J`, `Baseline_NonAtom_Ewma`, 3 stacking orders, and 41-arm structure remain 100% IDENTICAL**. Exactly four first-principle mechanical updates are frozen prior to editing code:

1. **Q2 replaced with `PhaseInterferometer` (Strictly Unitary `SO(2)` Chiral Spiral Mixing):**
   - **State:** Stateless unitary 2-arc Givens operator consuming `&mut CoinedWebWalk` and real mixing angle `theta_mix[16]` clamped to `[-pi/4, +pi/4]`.
   - **Intrinsic Mechanics:** Rotates counter-propagating spiral arcs `(d=0, d=1)` at each strand via exact unitary `(cos theta, sin theta)` Givens transform (requiring ZERO artificial renormalization), driven in `WebEngine` by inter-strand thermal bath gradient `((T_v - avg_nbr_T)/2 + (T_v - T_ambient)/4 + skew_v/8)`. Emits per-strand **chiral loop probability current** `chiral[v] = | |psi_{v,0}|^2 - |psi_{v,1}|^2 | + |Im(psi_{v,0}^* psi_{v,1})|` and `chiral_total = sum_v chiral[v]`.
   - **Invariant:** Exact unitary L2 norm preservation (`<= 1e-4`), zero amplitude concentration bias toward high-magnitude strands.
   - **Falsifier:** Zero-phase or `ClassicalWalkBase` (phase-blind) produces `chiral_total == 0`; `theta_mix == 0` recovers `Abl_NoPhaseInterferometer` identically.

2. **I1 (`InfoChannel`) Rate-Invariant Conditional Surprise:**
   - Adds per-strand rolling background surprise `bg_surp[16]` initialized at `SCALE` (1 bit) and initializes `surprise_state` to `[SCALE; 16]`.
   - On active observations: `inst = (-ln2(q_b)).max(0) + 2 * disp_dev`, updating `surprise_state[v] = (7 * prev + inst) / 8` and `bg_surp[v] = (63 * bg_surp[v] + surprise_state[v]) / 64`.
   - On inactive ticks: relaxes gently toward `bg_surp[v]` (`(63 * prev + bg_surp[v]) / 64`) instead of decaying toward `0`.
   - **Falsifier:** An active periodic/habitual 2-bin stream settles to strictly LOWER `surprise_mean` than a sparser uniform 8-bin random stream.

3. **M1 (`HarmonicWeb`) & `Early` Chaos Spatial-Shear Drive:**
   - Defines connected spatial shear `shear_signed[v] = act[v] - (act[cw] + act[ccw] + act[chord]) / 3` (falling back to `act[v] / 2` when disconnected).
   - Drives `HarmonicWeb` with `shear_signed[v] + (press[v] >> 1) + base_force[v] + info_mod[v] + prev_mid_delta[v]`.
   - Drives `Early` `LogisticChaos` (`X1`) with spatial shear magnitude `shear_abs[v] + disp[v]` rather than raw amplitude `mag[v] + press[v]`.

4. **X1 & X2 (`Chaos`) Normalized Bounded Perturbation Export:**
   - Keep exact `LogisticChaos` (coupled map lattice) and exact area-preserving symplectic `StandardMapChaos` (`p', theta'`).
   - Scale `LogisticChaos` exported `delta[v]` to `(u[v] - u_ema[v]) >> 3`.
   - Replace `StandardMapChaos` excursion tracker with fast kick-impulse EMA `kick_ema[16]` (`kick = mul(k_v, sin(theta_v))`, `centered = kick - kick_ema[v]`, `delta[v] = centered >> 3`), so perturbations relax immediately when `K_v` drops back below KAM breakup `K_c` during `R6_recovery`.

---

## 3. Frozen v2 41-Arm Matrix & Explicit Predictions / Falsifiers

- **Exact Run Count:** Identical **41 arms × 7 regimes × 3 seeds = 861 runs** (with arm #34 `Abl_NoWeakMeasurement` updated to `Abl_NoPhaseInterferometer` using `BaseMode::QuantumNoInterferometer`).
- **Precommitted v2 Predictions & Falsifiers:**
  1. **Principal Full Complex Stack vs Non-Atom Baseline:** With amplitude-collapse and firing-rate biases fixed, full 10-block `OrderA_NoChaos` (and promoted variants) will exceed `Baseline_NonAtom_Ewma` (`J = 0.6715`) while preserving all Hard Invariants `H1..H7`. *Falsifier:* Full 6-domain `OrderA_NoChaos` remains below `0.6715` or breaks any `H1..H7`.
  2. **Rule 4 Load-Bearing Quantum Base:** Full `OrderA_NoChaos` strictly exceeds BOTH `Ctrl_ClassicalUpperOnly` and `Ctrl_ReplacementBaseDiffusion` on `J`, `R3_phase_jitter`, and `WebProp`, and strictly passes `H5` where `Ctrl_BaseOnly` fails. *Falsifier:* Quantum base fails to improve `R3` or `J` over `ClassicalWalkReplacement`.
  3. **Rule 5 Chaos Search:** Across `Early`/`Mid`/`Late` single & multi-chaos arms, preserve all wins, ties, and regressions verbatim; promote a chaos-bearing candidate ONLY if it strictly beats its matched `NoChaos` and constituent single-chaos controls with `H1..H7` intact.
  4. **Rule 6 Required Domains:** Observe empirically how `Abl_NoPhaseInterferometer`, `Abl_NoMechanics`, `Abl_NoPhysics`, `Abl_NoInfo`, `Abl_NoChemistry`, `Abl_NoCognition`, and `Ctrl_DisconnectedWeb` behave relative to full `OrderA_NoChaos`.
