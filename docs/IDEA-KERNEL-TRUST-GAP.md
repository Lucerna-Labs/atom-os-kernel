# Idea: closing the kernel trust gap for secrets

Status: IDEA, from a conversation with Jesse on 2026-10-21. Nothing here is built. Each item below
is a possibility, with what is known and where the evidence is.

## Where the conversation started

**The question:** can a stored key be hidden so that a RAM snapshot can't capture it?

**The answers, in order:**
1. **The simulated thermal state lives in RAM.** In synthetic-brain, SCAR-1's scars (band energies
   and trap material) are numbers in RAM, 422,400 bytes for the whole medium.
   - No key is stored as a record. But a snapshot captures the whole medium, and a region's profile
     *is* its key, blurred by noise.
   - A skilled analyst could recover the keys in hours to days; with the code, instantly. This is
     obscurity, not protection (Kerckhoffs's principle: assume the attacker knows the design).
2. **More dimensions, or a keyed projection, only slow an attacker down.**
   - RAM is one line of bytes, and diffusion makes each axis's stride show up under
     autocorrelation.
   - A permutation keyed with a secret hides the layout, but the running program must hold that
     secret, so a full snapshot captures it too.
3. **A behavioural stack** (the secret as behaviour, not data: challenge–response) **can still be
   cloned in software.** A snapshot of code plus state, when run, answers like the original.
4. **The physical thermal state is different.** The Arc's real die heat is in the silicon, not in
   RAM. A snapshot captures the *readings* but not the chip's ability to produce its next responses:
   the physically unclonable function (PUF) idea, which includes thermal and ring-oscillator PUFs.
   - **Untested:** whether the A380's thermal response is unique and stable enough.
   - The sensors are coarse: 1 °C, and about 1 kHz of power readings.
5. **Jesse's no-copy stacks already exist** (see the no-copy memory note):
   - **Imperium's `encrypted key protection/`** (`nocopy-generator-core`, `nocopy-linear-map-core`,
     and their orchestrators) and quantum-network E27. Copying is *unreachable* by conservation
     (Move, Split, Merge of conserved charge) and by linearity (cloning is not linear).
   - **atom-coder's HOMEO-1/2:** a homeostatic immutable sensor *undoes* the copy actions it
     senses.
   - **hybrid-radio-waves' `atom-no-copy-stack`:** the quantum-base experiments.
   - **Neither copy-proof stack sees a RAM snapshot taken from outside the process,** because the
     snapshot is not one of their operations.
6. **Keys in RAM are normally plain.** Encryption at rest and in transit does not cover the working
   copy.
   - **Whole-RAM encryption** (Intel TME, AMD SME/SEV) is absent on this machine: the i7-13700K
     shows no tme, sme, sev or sgx flags.
   - **This machine has a TPM 2.0** (`/dev/tpm0`). Keys sealed in it are used inside the chip and
     never enter RAM: a hardware no-copy key store that is already here.
7. **The CPU can hold a key without extra hardware: registers only.**
   - TRESOR (2011) keeps an AES key in the debug registers and encrypts with AES-NI inside the CPU,
     so the key never touches RAM.
   - Follow-ups: Loop-Amnesia, and PRIME (RSA in the vector registers).
   - **The limits:** room for only a key or two; the kernel must cooperate (registers must not be
     saved to RAM at a context switch); and it stops snapshots and cold-boot attacks, but not a
     hostile kernel.
8. **The kernel is the trust root** (the trusted computing base). Whatever the kernel sees, a
   compromised kernel sees.
   - Below it are even more privileged layers: a hypervisor, firmware (UEFI, SMM), Intel ME, and
     devices with DMA.
   - **The remedies:** a smaller kernel (microkernels: seL4, Redox), and taking the kernel out of
     the trust chain for secrets (TPM, enclaves, encrypted VMs).

## What the two kernels have today (a read of the code, 2026-10-21)

**atom-os-kernel has the secret *lifecycle*:**
- E22, a fail-dead thermodynamic key (`kernel-key`);
- E23, an instant key from 4 XOR shares rebuilt inside one syscall (`kernel-instant`);
- E24, the egress gate (`kernel-egress`);
- E25, the honey key lane (`kernel-lane`);
- E35, the crypt layer (`kernel-crypt`);
- kernel-sense quarantine and intrusion destruction.

**The gaps (several are labelled honestly in the code itself):**
- **All keys are RAM statics.** That covers `kernel-key` :36 and the `kernel-instant` shares :37.
  `kernel-crypt` :29–40 says the master key "IS in RAM", with register residency down as "v2".
  E23's doc says reading all 4 slots defeats it.
- **It is monolithic,** with about 12.3k lines in ring 0, drivers included.
- **There is no IOMMU:** virtio-net enables bus mastering (`kernel-kit/src/virtio_net.rs:99`).
- There is no TPM, no capabilities, no enclave.
- The cipher is custom, and the seeds are deterministic (`scheduler.rs:82`).
- `encrypt_page` is never applied to real pages.

**redox-kernel-audit has the *isolation*:**
- a microkernel, with user-space drivers;
- the boot-verified, revocable **AuthorityWall** capability system built from `driver-primitives`
  (`docs/security-wall.md`), coupled to the sense field and a taint gate.

**Its gaps:** no key handling at all, and no IOMMU.

**So the two kernels each hold half of the answer.**

## Possibilities (see the to-do list)

1. **Registers-only key** (E35 v2, TRESOR-style): the master key in the debug or vector registers,
   with encryption by AES-NI inside the CPU and never stored in RAM. This needs the scheduler to
   leave those registers alone at a context switch.
2. **A TPM root share:** seal one share of the E23 key (or the E35 root) in the TPM, which only
   signs or unseals inside the chip.
3. **Conserved shares:** rebuild E23's 4 XOR shares on Imperium's no-copy algebra, so shares
   *move* (Move, Split, Merge of conserved charge) instead of sitting as copyable statics.
4. **Spread the shares across places a snapshot can't capture together:** CPU registers, the Arc's
   VRAM, the TPM, and a second machine.
5. **A physical anchor:** test whether the Arc's thermal response to a challenge pattern is a
   stable, chip-unique fingerprint (a thermal PUF). It may need the composed clock thermometer, and
   the GPU compute-kernel path is still on hold.
6. **A ratchet so copies go stale:** each use advances the state irreversibly, and one verified
   lineage detects a stale clone.
7. **Shrink the trust base:**
   - bring the secret lifecycle (E22–E25, E35, sense) onto redox-kernel-audit's capability base;
   - or move atom-os-kernel's drivers out of ring 0;
   - add IOMMU confinement so devices cannot read arbitrary RAM.
8. **Standard crypto for real secrets:** keep the custom cipher for experiments, and use AES-NI
   (and a real entropy source) where secrets matter.

## Jesse's direction (2026-10-21): protect the front door, not RAM

> "Not worried about ram, the key here is to protect the front door."

**The architecture he means:** a **synthetic brain**, immutable, acting as both kernel security and
network security. It is the **boundary organ** that decides what may **enter** and what may
**leave**. The RAM possibilities above are kept for reference, but they are not the goal.

**Why the front door is the right target:** if nothing leaves without the brain's judgment, a secret
sitting in RAM cannot be carried out by a remote attacker.

**What the immutable brain brings to the door:**
- **Immutable laws and identity:** its judgment cannot be rewritten. A change is a disturbance it
  returns from (HOMEO-1's law, scaled up), so there are no rules or thresholds for an attacker to
  edit.
- **Perception, not rule-matching:** sense → perceive (a named hypothesis: "a key-shaped payload
  leaving", "a new conversation") → judge, for every crossing.
- **A memory of normal:** scars of the machine's usual conversations, kept alive by use (SCAR-1's
  maintain-or-erase). The unfamiliar has no scar.
- **Temperature as vigilance:** T6's base temperature sets how rigid or how forgetful memory is,
  here a dial between "adapt to new normal" and "hold the line".
- **Both directions in one organ.**

**The pieces that already exist:**
- in this kernel: E24's egress cone, E25's key lane with honey, E37's network ingress ceremony,
  `kernel-net` with `kernel-lightcone`, and kernel-sense quarantine;
- elsewhere: atom-coder's HOMEO-2 (it beat a last-writer ARP cache against spoofers), and
  Imperium's VPN path with its no-copy observers.

**The open problems:**
1. **Covert channels:** data can leave through timing, or through heat (synthetic-brain BEAT-1 sent
   bits through the Arc's temperature). The door must sense side channels, not only packets.
2. **Encrypted traffic is opaque:** judge by behaviour (who, when, how much, what rhythm), not
   content.
3. **False positives:** E21's armed cone condemned the shell, so judgment must spare legitimate
   work.
4. **Learning against immutability (a question for Jesse):** are only the laws and identity
   immutable, with a learning part for new normal, or is the whole state fixed?

**In the field's words:**
- a *reference monitor* (one unbypassable judge for every crossing);
- *artificial immune systems* (Forrest, 1994: self against non-self);
- *data loss prevention* (egress control of secrets);
- the *blood–brain barrier* (a biological boundary organ).
