```text
   ██████╗  ██████╗  ██████╗ ██╗████████╗ █████╗ ████████╗ ██████╗ ██████╗
  ██╔════╝ ██╔═══██╗██╔════╝ ██║╚══██╔══╝██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
  ██║      ██║   ██║██║  ███╗██║   ██║   ███████║   ██║   ██║   ██║██████╔╝
  ██║      ██║   ██║██║   ██║██║   ██║   ██╔══██║   ██║   ██║   ██║██╔══██╗
  ╚██████╗ ╚██████╔╝╚██████╔╝██║   ██║   ██║  ██║   ██║   ╚██████╔╝██║  ██║
   ╚═════╝  ╚═════╝  ╚═════╝ ╚═╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝



Cogitator is a deterministic execution  
and witnessed-telemetry framework  
for scientifically auditable evaluation  
of autonomous cyber defence systems.

Autonomous cyber agents are increasingly powerful.  
But their evaluation is fragile.

Stochastic inference drifts.  
Tool interfaces introduce nondeterminism.  
Parallel execution scrambles ordering.  
Logs are easy to fake after the fact.

Cogitator treats an evaluation run  
as a deterministic program.

The full event trace is committed  
to a cryptographic witness root.

This enables rebuildable, replayable,  
third-party verification.

---

## Core invariant

Same environment  
+ same input  
+ same seed  

→ same trajectory  
→ same witness root

Cogitator replaces narrative logs  
with verifiable execution commitments.

---

## Key contributions

Cogitator provides:

- Deterministic execution kernel  
  for agentic cyber evaluation  

- Cryptographic witness chains  
  committing to full causal traces  

- Explicit entropy budgeting  
  to make randomness measurable  

- Reproducible evaluation environments  
  grounded in NixOS derivations  

- Standalone verification of traces  
  via witness recomputation  

---

## Witness chains

Every execution event updates  
a sequential cryptographic commitment:

h_0     = BLAKE3(“COGITATOR” || witnessed_metadata)
h_{t+1} = BLAKE3(h_t || encode(event_t))

The final value is:

`witness_root = h_T`

It commits to the entire run history.
Provenance (created_at, toolchain versions, git metadata)
is recorded in meta.json but excluded from the witness root.

Any insertion, deletion, mutation,  
or reordering changes the witness root.

---

## Entropy budgeting

Agent evaluations often hide randomness.

Sampling temperatures.  
Planner branches.  
Tool timeouts.  
Scheduler jitter.

Cogitator treats randomness  
as an audited resource.

- entropy sources declared in witnessed metadata  
- consumption recorded in the trace  
- evaluations comparable across models  

Randomness becomes measurable.  
Not implicit.

---

## Reproducibility via NixOS

Cogitator is designed  
for reproducible NixOS environments.

This enables:

- pinned dependency graphs  
- hermetic toolchains  
- rebuildable experiments  
- bit-identical evaluation pipelines  
- third-party verifiable re-execution  

Published results can be reproduced  
from a flake lock.

They can be verified  
by recomputing the witness root.

---

## Artifact bundle (planned)

The Cogitator release will include:

- Nix flake pinning dependencies  
  and runtimes  

- Deterministic kernel  
  and tool wrappers  

- Canonical trace schema specification  

- Standalone verifier  
  for witness root recomputation  

- Regression suite  
  demonstrating stable witness roots  

---

## Threat model

Cogitator addresses:

- accidental nondeterminism  
  (parallelism, scheduling drift)  

- post-hoc log editing  
  or trace fabrication  

Cogitator does not defend against:

- fully malicious host substrates  
  (compromised hypervisors or OS)  

The goal is scientific auditability  
under declared pinned environments.

---

## Status

Cogitator is an active research project.

This repository is intended  
as the reference implementation  
accompanying the Cogitator paper  
and artifact release.

---

## References

- Guo et al.  
  *R2: Record and Replay at the Application Level.*  
  OSDI 2008  

- Malka et al.  
  *Functional Package Management Enables Reproducible Builds at Scale.*  
  arXiv 2025  

- Aumasson et al.  
  *The BLAKE3 Hashing Framework.*  
  IETF draft 2024  

- Trillian  
  Merkle-tree-backed verifiable logs  
  transparency.dev  

---

## License

MIT
