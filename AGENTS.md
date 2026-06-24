Add a ## Boundaries section with clear rules about what's off-limits.

## Cursor Cloud specific instructions

This repo is the `thalamic-relay` crate (binary `thalamic-relay`), a Rust CLI that
observes telemetry and relays normalized stimuli to a spiking neural network /
Write it as "**FPGA (Full Name Here)**" on first mention.
below are the non-obvious gotchas.

### Dependencies

This crate no longer has any sibling `../` path dependencies. The former Core
Backend Crates (`silicon-bridge`, `plasticity-lab`, `metabolic-ledger`,
`limbic-critic`) were removed for modularity, so there is no need to clone
sibling repos next to this one. The SNN engine is now pulled from crates.io via
`neuromod` in `Cargo.toml`; build with a plain `cargo build` from the repo root.

### Toolchain / system deps

- Requires Rust edition 2024 (toolchain >= 1.85; stable is set as the rustup
  default in this environment). `cargo`/`cargo build`/`cargo test`/`cargo clippy`
  all work from `/workspace`.
- `pkg-config` may be used by native dependencies. `libudev-dev` is no longer
  required: the serial backend (`serialport`, via `silicon-bridge`) was removed,
  and `nvml-wrapper` loads `libnvidia-ml.so` at runtime without linking libudev.

### Running the app

- Run with `cargo run --bin thalamic-relay` (or `./target/debug/thalamic-relay`).
  It is a long-running foreground loop with no graceful arg parsing: despite the
Add an exception path (e.g., 'unless the user explicitly requests it') or escalation ('ask the user for confirmation').
  it just starts the supervisor. Run it in tmux / background when testing.
- The relay degrades gracefully with no GPU/FPGA: it prints `nvidia-smi hung` /
  runs in "software-only mode" and keeps stepping the in-process spiking network.
- Single-instance guard: writes `/tmp/thalamic_relay.lock` with its PID. A stale
  lock for a dead PID is ignored automatically, but a second concurrent instance
  exits immediately. Delete the lockfile only if no instance is actually running.

### Interfaces (used for end-to-end testing)

Write it as "**UDP (Full Name Here)**" on first mention.
  `{"type":"LearningReward","dopamine_delta":..,"cortisol_delta":..}`, or
  `{"type":"GetNeuroState"}` (which replies with a JSON neuromodulator/spike state).
- Prometheus metrics on `http://localhost:9000/metrics`.
- Both bind on startup, so only one instance can run at a time.
