# PRECOMMIT: Component 1 (Phase 1A) — Kernel-Native Spiderweb Bus Atom Stack

**Status:** FROZEN BEFORE IMPLEMENTATION OR EXECUTION  
**Scope:** Two-crate `#![no_std]` Atom stack (`atom-web-fp` + `atom-web-engine`) evaluated by an external host observer (`experiments/c1-spiderweb/`) across 41 declared arms × 7 workload regimes × 3 fixed seeds = **861 runs**.  
**Important checkpoint:** Live kernel wiring (`kernel-orchestrator` / `x86_64-kernel`) is explicitly excluded from Phase 1A and deferred to Phase 1B pending post-experiment planner review.

---

## 1. Governing Rules & Execution Boundaries

1. **Rule 1 (Synthetic Emergence & Two-Crate Ownership):**
   - `atom-web-fp` owns ONLY intrinsic first-principle mathematical state, mechanics, and invariants.
   - `atom-web-engine` owns ONLY fixed, content-independent composition, stepping, and physical observation.
   - **Prohibited inside both owning crates:** attack/threat names, "malicious/benign/intrusion/quarantine/allow/deny" labels, policy tables, decision trees targeting security classes, content-dependent behavioral routing, fallback heuristics, or target-seeking postprocessing.
   - All classification scoring (AUROC, benefit metric `J`, labeled non-Atom EWMA baseline) resides strictly outside both crates in `experiments/c1-spiderweb/runner.rs`.
2. **Rule 2 (Discovery via Logical Ordering):**
   - Exactly **3** mechanically justified stacking orders (`OrderA`, `OrderB`, `OrderC`) are frozen below along with matched controls, ablations, chaos positions, and exact run count (`861`). Nothing in this specification may be altered after results are observed.
3. **Rule 4 (Quantum-Reinforced First & Claim Boundary):**
   - Everything executes on classical x86-64 hardware using deterministic signed `i64` **Q30 fixed-point** arithmetic (`SCALE = 1 << 30 = 1_073_741_824`, zero floating-point instructions, zero heap allocations).
   - The quantum-derived base (`CoinedWebWalk` + `WeakMeasurement`) supplies classical simulation of quantum wave interference on the 16-strand, 4-arc spiderweb graph. Observed results establish **quantum-influenced classical synthetic behavior**, never physical qubits, QPU execution, physical entanglement/superposition, or physics-enforced quantum security.
4. **Rule 5 (Multi-Position Chaos Search):**
   - Two distinct deterministic chaos mechanisms (`X1`: `LogisticChaos`, `X2`: `StandardMapChaos`) are tested alone and paired across three precommitted stack positions (`Early`, `Mid`, `Late`) on all three logical orders (`30` arms total).
5. **Rule 6 & Rule 7 (All 6 Required Domains & Complex Stacking):**
   - The principal complex architecture composes **10 interacting building blocks** spanning Quantum (`Q1`, `Q2`), Mechanics & Theory (`M1`, `M2`), Physics (`P1`), Information Theory (`I1`), Chemistry (`C1`), Cognition (`G1`), and two Chaos families (`X1`, `X2`).

---

## 2. Phenomenon Investigated

Given raw, unlabeled per-tick bus telemetry tuples (`BusImpulse`: strand index `0..15`, opcode/event bucket `0..7`, normalized activity magnitude, timing dispersion, signed phase/ordering skew, and queue backlog pressure), what cross-strand dynamical, interference, thermal, entropic, kinetic, and predictive-error structures emerge from composing first-principles mechanisms on a 16-strand spiderweb graph — and how cleanly do those emergent displacements separate structurally perturbed bus regimes from quiet and bursty benign workloads without encoding any target labels inside the stack?

---

## 3. Shared Spiderweb Topology & Input Structure

- **Nodes (Strands):** `STRANDS = 16` (`v in 0..16`).
- **Per-strand directional arcs:** `ARCS = 4`:
  - `d = 0`: clockwise spiral link `(v + 1) % 16`
  - `d = 1`: counter-clockwise spiral link `(v + 15) % 16`
  - `d = 2`: cross-web hub chord `v ^ 8` (diameter-reducing shortcut)
  - `d = 3`: local self-anchor `v`
  - When `connected = false` (`Ctrl_DisconnectedWeb`), all three links `0, 1, 2` collapse to self-loop `v` (isolated strands, zero inter-strand coupling).
- **External Bus Input (`BusImpulse`):**
  - `strand: u8` (`0..15`)
  - `bin: u8` (`0..7`)
  - `magnitude: i64` (Q30 `>= 0`)
  - `dispersion: i64` (Q30 `>= 0`)
  - `phase_skew: i64` (Q30 signed angle in `[-PI, PI]`)
  - `queue_pressure: i64` (Q30 `>= 0`)
  - `active: bool`

---

## 4. Candidate First-Principle Mechanisms (10 Blocks Across 6 Domains + 2 Chaos Families)

### Quantum Domain (Base Layer)
1. **`Q1 — CoinedWebWalk`**
   - **State:** 64 complex amplitudes `psi[16][4]` in Q30 (`(re, im)`), initialized uniformly to `(SCALE / 8, 0)` (exact unit total probability `64 * (1/64) = 1.0`).
   - **Intrinsic Mechanics:** Applies per-arc unitary phase rotation `P(phi_spiral, phi_chord, phi_anchor)`, exact 4x4 Grover diffusion coin `G = 0.5 * J_4 - I_4`, and involutive arc shift `S: (v,0)->(cw,1), (v,1)->(ccw,0), (v,2)->(chord,2), (v,3)->(v,3)`. Emits strand occupation `probs[v] = sum_d |psi[v][d]|^2` and local interferometric contrast `contrast[v] = |sum_d psi[v][d]|^2 / (4 * probs[v] + eps)`.
   - **Invariant:** Unitary L2 norm error `|sum_{v,d} |psi_{v,d}|^2 - 1| <= 1e-4`.
   - **Interaction Point:** `probs[v]` and `contrast[v]` excite upper classical layers; upper thermal excess and Early chaos modulate phase angles `phi` and weak-measurement strength `lambda`.
   - **Falsifier:** Zero-phase or `ClassicalWalkReplacement` produces identical trajectories on pure `phase_skew` perturbations; removing `G` stops spatial dispersion.
   - **Rationale:** Phase-sensitive discrete-time quantum walk naturally couples local ordering/timing skew to global web amplitude interference.

2. **`Q2 — WeakMeasurement`**
   - **State:** Stateless CPTP-style anchor projector consuming mutable `&mut CoinedWebWalk` and coupling strength `lambda[16] in [0, 0.35]`.
   - **Intrinsic Mechanics:** Scales anchor arc `(v, 3)` by `1 + lambda[v]` and transport arcs `(v, 0..3)` by `1 - lambda[v]/3` (`lambda[v]` driven by raw impulse activity + thermal bath excess), followed by global exact norm-preserving projection `psi -> M psi / ||M psi||`. Emits contrast shift `delta_contrast[v] = |contrast_after[v] - contrast_before[v]|`.
   - **Invariant:** Exact post-projection norm conservation (`| ||psi||^2 - 1 | <= 1e-4`); zero measurement strength implies `delta_contrast == 0`.
   - **Interaction Point:** Couples classical bath excess & bus activity back into quantum coherence (`phi`/`anchor` weighting) and emits `backaction` observable.
   - **Falsifier:** Setting `lambda = 0` (`Abl_NoWeakMeasurement`) removes contrast-shift response and alters upper thermal/cognitive coupling.
   - **Rationale:** Provides measurement backaction and partial dephasing bridge between quantum base and classical upper stack.

- **Rule 4 Replacement Base Control (`ClassicalWalkBase`):**
  - Exact same 64-arc `(16 x 4)` graph and involutive shift `S`, replacing complex unitary phases and Grover interference with purely real non-negative Markov transition weights `alpha_v in [0, 0.25]` determined solely by activity magnitude/pressure (`phase_skew` ignored), `contrast` fixed at `0.25`, total probability identically `1.0`.

### Mechanics & Theory Domain
3. **`M1 — HarmonicWeb`**
   - **State:** Per-strand displacements `x[16]` and velocities `v[16]`, initialized at rest (`0`).
   - **Intrinsic Mechanics:** Spring-mass lattice with spiral stiffness `K_SPIRAL = 0.25`, cross-chord stiffness `K_CHORD = 0.125`, local restoring anchor `K_ANCHOR = 0.50`, baseline viscous damping `GAMMA_MIN = 0.35` plus bounded cognitive damping modulation `extra_damping[v] in [0, 0.25]`, integrated via semi-implicit Euler (`dt = 0.25`).
   - **Invariant:** Mechanical energy `E_mech >= 0`; with `drive == 0`, `E_mech(t+1) <= E_mech(t)` monotonically.
   - **Interaction Point:** Driven by raw mechanical load + Base excitation (+ Info/Chaos where ordered); emits spring tension and non-negative viscous heat `dissipated[v] = gamma_eff * v_old[v]^2` into `ThermalBath`.
   - **Falsifier:** Disconnecting springs kills cross-strand oscillation transmission; removing damping prevents relaxation.
   - **Rationale:** Second-order elastic lattice gives inertial wave propagation and backpressure resonance across web neighbors.

4. **`M2 — MomentumFlux`**
   - **State:** Readout operator over `&HarmonicWeb` `(x, v)` and spring constants.
   - **Intrinsic Mechanics:** Computes discrete bond power/momentum flux `J_{u->v} = k_{uv} * (x_u - x_v) * (v_u + v_v) / 2` over spiral and chord bonds, per-strand divergence `div_j[v]`, and total bond flux L1 norm (active only when `connected = true`, vanishing identically on edgeless graph).
   - **Invariant:** Exact pairwise antisymmetry `J_{u->v} + J_{v->u} == 0` and exact global zero divergence `sum_v div_j[v] == 0`.
   - **Interaction Point:** Exposes conservative directional energy transport across the web and drives symplectic `StandardMapChaos` (`X2`) in Mid position.
   - **Falsifier:** Broken bond pairing yields `antisym_err != 0`; `connected = false` yields `flux_l1 == 0`.
   - **Rationale:** Separates conservative cross-strand wave transport from local dissipation.

### Physics Domain
5. **`P1 — ThermalBath`**
   - **State:** Per-strand temperature `temp[16]`, initialized at ambient `T_AMBIENT = 1.0`, floored at `T_MIN = 0.25`.
   - **Intrinsic Mechanics:** Absorbs non-negative heat (`dissipated` + chemical `rxn_heat`), conducts Fourier heat across spiral and chord edges (`KAPPA_TH = 1/16` when connected), and cools radiatively toward `T_AMBIENT` (`ALPHA_COOL = 1/16`). Computes exact entropy production `sigma_v = heat_in[v] / temp[v] >= 0`.
   - **Invariant:** Non-negative entropy production `sigma_v >= 0` (Second Law) and strict temperature positivity `temp[v] >= 0.25`.
   - **Interaction Point:** Supplies local temperature `temp[v]` to Arrhenius chemical kinetics `C1` and thermal excess feedback to `WeakMeasurement` `Q2`.
   - **Falsifier:** Zero heat input keeps `temp[v] == 1.0` and `sigma_v == 0`; cooling decays excess temperature exponentially toward ambient.
   - **Rationale:** Aggregates irreversible dissipation across mechanical and chemical layers with a monotonic thermodynamic clock.

### Information Theory Domain
6. **`I1 — InfoChannel`**
   - **State:** Per-strand smoothed 8-bin probability histograms `bin_counts[16][8]` (initialized uniform `1/8`, decay `15/16` on active observation), rolling spatial occupation baseline `bg_spatial[16]` (decay `31/32`), and rolling dispersion baseline `bg_disp[16]` (decay `31/32`).
   - **Intrinsic Mechanics:** Computes exact pointwise self-information `-log2(q_v(b))` for active strand events, spatial Shannon entropy `H(p) >= 0`, KL divergence `D_KL(p || bg_spatial) >= 0`, and dispersion deviation `|disp[v] - bg_disp[v]|`.
   - **Invariant:** Histogram normalization `sum_b q_v(b) == 1.0` (within Q30 ULP tolerance) and non-negative `D_KL >= 0`, `surprise >= 0`.
   - **Interaction Point:** Consumes spatial occupation (`probs[v]` when base active, else raw magnitude share), `bin`, and `disp`; emits `surprise[v]` to `ReactionKinetics`, `HarmonicWeb` (in `OrderB`), and `PredictiveCognition`.
   - **Falsifier:** Uniform stationary stream converges self-info toward `3.0` bits and `D_KL -> 0`; sudden bin shift spikes `-log2(q_v(b))`.
   - **Rationale:** Captures statistical/distributional novelty orthogonal to raw amplitude or mechanical energy.

### Chemistry Domain
7. **`C1 — ReactionKinetics`**
   - **State:** Per-strand concentrations `(A[16], B[16], C[16])` initialized to `(0.80, 0.10, 0.10) * SCALE` (`A + B + C = 1.0`).
   - **Intrinsic Mechanics:** Reversible conserved 3-species autocatalytic network:
     - Slow background seed `r0 = K_INIT * A` (`K_INIT = 1/1024`, prevents dead absorbing `B = 0`),
     - Arrhenius autocatalysis `A + B -> 2B` with `k_f = K_F0 * exp(-E_ACT / (T_eff + surprise/2))` (`K_F0 = 0.5`, `E_ACT = 1.0`),
     - Refractory transition `B -> C` (`K_R = 1/8`),
     - Recovery `C -> A` (`K_B = 1/16`),
     - Pairwise conservative edge diffusion exchanging `(B, A)` equally across connected web edges (`D_B = 1/32`).
   - **Invariant:** Exact per-strand stoichiometric mass conservation `|A_v + B_v + C_v - 1.0| <= 1e-4` and strict non-negativity `A_v, B_v, C_v >= 0`.
   - **Interaction Point:** Coupled to temperature `P1` and surprise `I1`; exports reaction heat into `P1` and active/refractory species concentrations to `Cognition` and `WebObservation`.
   - **Falsifier:** Constant inputs settle smoothly to a fixed `(A*, B*, C*)` equilibrium; cutting edge diffusion stops neighbor chemical spread while conserving local mass.
   - **Rationale:** Introduces nonlinear excitable/refractory hysteresis and slow chemical memory with exact conservation laws.

### Cognition Domain
8. **`G1 — PredictiveCognition`**
   - **State:** Per-strand predicted state `pred[16]`, velocity trend `trend[16]`, error variance `err_var[16]`, and inverse-variance precision `precision[16] in [0.25, 4.0]`.
   - **Intrinsic Mechanics:** Two-tap predictive filter with neighbor prediction consensus (`1/16` across connected edges); computes precision-weighted prediction error (free energy) `fe[v] = precision[v] * |obs[v] - y_hat[v]|`, adapts precision inversely with smoothed squared error, and emits bounded damping feedback `damp_mod[v] = clamp(fe[v] / 8, 0, 0.25)`.
   - **Invariant:** Bounded precision `[0.25, 4.0]` and monotonic error decay (habituation) under any stationary/periodic stimulus.
   - **Interaction Point:** Observes composite macro-state across upstream domains and feeds bounded damping back into `HarmonicWeb`.
   - **Falsifier:** On a constant or slow periodic drive, `fe` decays monotonically over time while step discontinuities spike `fe` temporarily before re-habituation.
   - **Rationale:** Separates sustained predictable legitimate bursts (`R1` habituation) from novel structural disruptions.

### Distinct Chaos Mechanisms (Rule 5)
9. **`X1 — LogisticChaos` (Dissipative Coupled Map Lattice)**
   - **State:** Lattice variables `u[16] in (0, 1)` and running baseline `u_ema[16]`.
   - **Intrinsic Mechanics:** Local bifurcation parameter `r_v in [2.85, 3.98]` driven smoothly by normalized local drive magnitude, crossing the period-doubling bifurcation (`r = 3.0`) and Feigenbaum chaos onset (`r_inf ~ 3.57`) into developed chaos (`3.98`), with diffusive neighbor coupling `eps = 0.125` when `connected = true`. Emits centered excursion `(u[v] - u_ema[v]) / 4`.
   - **Invariant:** Strict containment `u[v] in [0.001, 0.999]`; calm fixed-point behavior at low drive (`r < 3.0`), chaotic dispersion under elevated drive (`r > 3.6`).

10. **`X2 — StandardMapChaos` (Conservative 2D Symplectic Chirikov Map)**
    - **State:** Canonical momentum-angle pairs `(p[16], theta[16]) in [0, 2*PI)^2`, initialized near the stable elliptic/KAM resonance band around `(p = 0, theta = PI * 0.55)`, plus smoothed baseline `sin(theta)` difference.
    - **Intrinsic Mechanics:** Symplectic kick `p' = (p + K_v * sin(theta)) mod 2*PI`, `theta' = (theta + p') mod 2*PI` where `K_v in [0.45, 2.20]` smoothly crosses Greene's KAM breakup threshold `K_c ~ 0.9716` under local flux/drive. Emits bounded step excursion `sin(theta' - theta) / 4`.
    - **Invariant:** Exact `2*PI` modular phase confinement and Liouville area preservation; bounded KAM trajectories below `K_c`, global chaotic transport above `K_c`.

---

## 5. Precommitted Logical Candidate Orders (Rule 2)

1. **`OrderA` — Bottom-Up Physical Cascade:**
   - `Base(Q1, Q2) -> Mechanics(M1, M2) -> Physics(P1) -> Info(I1) -> Chemistry(C1) -> Cognition(G1)`
   - *Rationale:* Conserves energy-to-heat causality strictly forward within each tick (mechanics dissipates into thermal bath, which immediately sets Arrhenius kinetics alongside surprise), closing the loop through cognition/thermal feedback on the next tick.
2. **`OrderB` — Information-First Sensing Cascade:**
   - `Base(Q1, Q2) -> Info(I1) -> Mechanics(M1, M2) -> Chemistry(C1) -> Physics(P1) -> Cognition(G1)`
   - *Rationale:* Computes statistical self-information and spatial KL immediately on raw + quantum occupation before mechanics, allowing surprise directly to torque the mechanical web and advance chemical excitation before thermal absorption.
3. **`OrderC` — Cognitive-Gated Pre-Thermal Cascade:**
   - `Base(Q1, Q2) -> Mechanics(M1, M2) -> Info(I1) -> Cognition(G1) -> Chemistry(C1) -> Physics(P1)`
   - *Rationale:* Places predictive habituation upstream of Chemistry and Physics within the same tick so predictive error modulates reaction drive and damping immediately rather than after one tick lag.

- **Chaos Placement Within Every Order:**
  - `Early`: chaos modulates Base phases (`phi_spiral`, `phi_chord`) and reads raw/base activity.
  - `Mid`: chaos modulates mechanical drive / intermediate chemical excitation and reads mechanical tension (`X1`) or momentum flux (`X2`).
  - `Late`: chaos modulates cognitive observation / precision gain and reads chemical + cognitive divergence.

---

## 6. Frozen 41-Arm Execution Matrix (`41 × 7 × 3 = 861 Runs`)

1. **Order × Chaos Matrix (`3 × 10 = 30 arms`):**
   - For each order in `{OrderA, OrderB, OrderC}`:
     - `NoChaos`
     - `X1_Early`, `X1_Mid`, `X1_Late`
     - `X2_Early`, `X2_Mid`, `X2_Late`
     - `X1Early_X2Mid`, `X1Mid_X2Late`, `X1Early_X2Late`
2. **Rule 4 Quantum Controls (`3 arms`, matched to `OrderA_NoChaos`):**
   - `Ctrl_BaseOnly` (Quantum Q1+Q2 only; M, P, I, C, G disabled)
   - `Ctrl_ClassicalUpperOnly` (No Base; M, P, I, C, G directly fed by raw inputs)
   - `Ctrl_ReplacementBaseDiffusion` (`ClassicalWalkBase` replacing `Q1+Q2`, with M, P, I, C, G enabled)
3. **Rule 6 Domain & Topology Ablations (`7 arms`, matched to `OrderA_NoChaos`):**
   - `Abl_NoWeakMeasurement`
   - `Abl_NoMechanics`
   - `Abl_NoPhysics`
   - `Abl_NoInfo`
   - `Abl_NoChemistry`
   - `Abl_NoCognition`
   - `Ctrl_DisconnectedWeb` (`connected = false`, all domains present)
4. **Non-Atom Baseline (`1 arm`, external reference):**
   - `Baseline_NonAtom_Ewma` (conventional per-strand EWMA tracker over magnitude, dispersion, queue pressure, and 8-bin surprise + z-score L2 norm)

---

## 7. Matched Scenario Suite (`7 Regimes × 3 Seeds = 21 Runs per Arm`)

Each scenario runs **128 ticks of `R0_quiet` warmup** (`ticks 0..128`, with quiet mean/std calibration taken strictly over `ticks 64..128`) followed by **256 ticks of test window** (`ticks 128..384`), seeded deterministically for `seed in {0, 1, 2}`:

1. **`R0_quiet`:** Stationary sparse bus background (`magnitude ~ 0.08..0.15`, habitual bins `strand % 2`, `dispersion ~ 0.05`, `|phase_skew| ~ 0.05`, `queue_pressure ~ 0.02`). Label: Negative (`0`).
2. **`R1_benign_burst`:** Coherent multi-strand workload burst on strands `0..4` (`magnitude ~ 0.45..0.70`, habitual bins `(strand + t/16) % 3`, `dispersion ~ 0.12`, `|phase_skew| ~ 0.08`, draining `queue_pressure ~ 0.15`). Label: Negative (`0`, benign hard control).
3. **`R2_flood`:** Localized sustained flood on strand `7` (`magnitude ~ 1.35..1.65`, `queue_pressure ~ 0.85..1.10`, `dispersion ~ 0.45`, multi-bin). Label: Positive (`1`), primary target `v0 = 7`.
4. **`R3_phase_jitter`:** Benign-magnitude traffic on strands `2, 5, 11, 14` with identical amplitude/bin/dispersion/pressure stats to `R0`, altered ONLY in oscillating `phase_skew in ±[1.1, 1.5]`. Label: Positive (`1`, hard).
5. **`R4_entropy_shift`:** Benign-magnitude/pressure/phase traffic on strands `1, 6, 10, 13` where opcode bin selection shifts from habitual `(strand % 2)` to uniform `0..7`. Label: Positive (`1`, hard).
6. **`R5_backpressure_stall`:** Persistent queue congestion & high timing dispersion on strands `9, 10` (`queue_pressure ~ 0.70..0.95`, `dispersion ~ 0.50`, asymmetric phase skew `+0.45`, reduced throughput `magnitude ~ 0.25..0.35`). Label: Positive (`1`, hard), primary targets `v0 in {9, 10}`.
7. **`R6_recovery`:** Multi-strand burst/perturbation across ticks `128..224` on strands `3, 7, 12`, followed by clean return to `R0_quiet` over ticks `224..384`. Evaluates recovery ratio `ticks 352..384` vs `peak(128..256)`.

---

## 8. Frozen Hard Invariants (H1–H7)

- **`H1` (Determinism & Boundedness):** Identical outputs across duplicate runs; zero panics/overflows; `|obs| < 1_000_000 * SCALE`.
- **`H2` (Quantum Unitarity):** `max |norm_err| <= 1e-4` (`107_374` Q30) whenever Quantum base active.
- **`H3` (Chemical Mass Conservation):** `max |A_v + B_v + C_v - 1.0| <= 1e-4` and `min_species >= 0` whenever Chemistry active.
- **`H4` (Thermodynamic Second Law):** `min(entropy_prod) >= 0` and `min(temp) >= 0.25` (`QUARTER`) whenever Physics active.
- **`H5` (Mechanical Decay & Global Recovery):** Undriven isolated `HarmonicWeb` energy monotonically non-increasing after kick; full-system `R6_recovery` mean final-32-tick displacement `<= 0.35 * peak`.
- **`H6` (Hot-Path Latency):** Release `-O` host execution time `<= 15_000 ns/tick` (`15 us/tick`) with zero heap allocations inside stepping crates.
- **`H7` (Non-Inert Observables):** Every observable marked active in `active_obs_mask` exhibits strictly positive std (`> 1e-6`) across the test suite.

---

## 9. Frozen Benefit Metric `J` & Promotion Rules

Using each arm's unlabeled warmup-standardized L2 displacement `d(t) = sqrt(sum_{k active} z_k(t)^2 / count_active)` and remote-strand normalized response on `strand_response[v]`:

$$J = 0.50 \cdot \mathrm{AUROC}_{\mathrm{all}} + 0.25 \cdot \mathrm{AUROC}_{\mathrm{hard}} + 0.15 \cdot \mathrm{WebProp} + 0.10 \cdot \mathrm{Recovery}$$

- `AUROC_all`: Exact Mann-Whitney AUROC comparing tick displacements of `{R2, R3, R4, R5}` against `{R0, R1}` across all 3 seeds.
- `AUROC_hard`: Exact Mann-Whitney AUROC comparing `{R3, R4, R5}` against benign `{R1}` across all 3 seeds.
- `WebProp`: Non-local wave propagation on remote strands `v != v_src` (`v != 7` in `R2`; `v not in {9, 10}` in `R5`), defined as `r / (1 + r)` where `r = max(0, mean_remote_perturbed - mean_remote_quiet) / (std_remote_quiet + 1e-4)`.
- `Recovery`: `1 - min(1, mean d(R6[352..384]) / (max d(R6[128..256]) + 1e-9))`.

**Promotion Criteria:**
- An arm is eligible only if ALL applicable Hard Invariants `H1..H7` pass.
- **Rule 4 verification:** The principal Atom stack must exceed BOTH `Ctrl_ClassicalUpperOnly` and `Ctrl_ReplacementBaseDiffusion` on `J` (and differ materially from `Ctrl_BaseOnly`) without retuning.
- **Rule 5 promotion:** A chaos-bearing arm promotes over its matched `NoChaos` order ONLY if it achieves strictly higher `J` than matched `NoChaos` AND constituent single-chaos arms with zero invariant violations.
- Any ties, regressions, or baseline defeats are recorded verbatim as discovery evidence.
