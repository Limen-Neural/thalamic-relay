## Cursor Cloud specific instructions

This repo is the `thalamic-relay` crate (binary `thalamic-relay`), a Rust CLI that
observes telemetry and relays normalized stimuli to a spiking neural network and
**FPGA (Field-Programmable Gate Array)** / network backends.
Below are the non-obvious gotchas.

### Dependencies

This crate no longer has any sibling `../` path dependencies. The former Core
Backend Crates (`silicon-bridge`, `plasticity-lab`, `metabolic-ledger`,
`limbic-critic`) were removed for modularity, so there is no need to clone
sibling repos next to this one. The spiking neural network (SNN) engine is now
pulled from crates.io via `neuromod` in `Cargo.toml`; build with a plain
`cargo build` from the repo root.

### Toolchain / system deps

- Requires Rust edition 2024 (toolchain >= 1.87 for `u64::is_multiple_of` and
  clippy lints used in CI; stable is set as the rustup default in this
  environment). `cargo`/`cargo build`/`cargo test`/`cargo clippy` all work from
  `/workspace`.
- `pkg-config` may be used by native dependencies. `libudev-dev` is no longer
  required: the serial backend (`serialport`, via `silicon-bridge`) was removed,
  and `nvml-wrapper` (NVML — NVIDIA Management Library) loads `libnvidia-ml.so` at runtime without linking libudev.

### Running the app

- Run with `cargo run --bin thalamic-relay [OPTIONS]` (or the installed binary).
  It is a long-running supervisor. Use `--help` for options (or the equivalent
  THALAMIC_* environment variables). The supervisor now supports graceful arg
  parsing via clap (derive + env features; implemented for #11). Run it in tmux /
  background when testing (unless the user explicitly requests foreground behavior).
- The relay degrades gracefully with no GPU/FPGA: it prints `nvidia-smi hung` /
  runs in "software-only mode" and keeps stepping the in-process spiking network.
- Single-instance guard: writes `/tmp/thalamic_relay.lock` with its PID. A stale
  lock for a dead PID is ignored automatically, but a second concurrent instance
  exits immediately. Delete the lockfile only if no instance is actually running.

### Interfaces (used for end-to-end testing)

- **UDP (User Datagram Protocol)** on `127.0.0.1:9898` (or your --udp-addr; newline-free JSON):
  `{"type":"LearningReward","dopamine_delta":..,"cortisol_delta":..}`, or
  `{"type":"GetNeuroState"}` (which replies with a JSON neuromodulator/spike state).
- Prometheus metrics on `http://localhost:9000/metrics` (bind IP (Internet Protocol) configurable via --metrics-ip).
- Both bind on startup, so only one instance can run at a time.

## Boundaries

- **Owns**: `src/`, `Cargo.toml`, `README.md`, `AGENTS.md`
- **Does Not Own**: sibling crates, `neuromod` upstream
- **Off-limits**: do not edit sibling path-dependency crates, do not introduce mining/trading domain logic
