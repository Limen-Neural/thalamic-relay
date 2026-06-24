# AGENTS.md

## Cursor Cloud specific instructions

This repo is the `thalamic-relay` crate (binary `thalamic-relay`), a Rust CLI that
observes telemetry and relays normalized stimuli to a spiking neural network /
FPGA backend. Standard commands live in `README.md` and `Cargo.toml`; the notes
below are the non-obvious gotchas.

### Sibling path dependencies (most important)
`Cargo.toml` uses `../` path dependencies, so these crates MUST exist as siblings
of the repo (i.e. at `/silicon-bridge`, `/plasticity-lab`, `/metabolic-ledger`,
`/limbic-critic`, since the repo is checked out at `/workspace`). They are cloned
from the `Limen-Neural` GitHub org. The startup update script recreates any that
are missing, so normally you do not need to touch them. If a build fails with
"failed to load source for dependency", confirm those four directories exist.
Note the `plasticity-lab` repo's package name is intentionally misspelled
`plascticity-lab` — that is expected, not a typo to fix.

The checkout location (`/workspace`) is fixed by the Cloud environment, and the
`../` paths in `Cargo.toml` force the siblings to live in `/`, which is NOT
persisted across VM rebuilds (only `/workspace` is). That non-persistence is
expected and harmless here: the idempotent startup update script re-clones any
missing siblings on every run, so they are always present before a build.

### Toolchain / system deps
- Requires Rust edition 2024 (toolchain >= 1.85; stable is set as the rustup
  default in this environment). `cargo`/`cargo build`/`cargo test`/`cargo clippy`
  all work from `/workspace`.
- Native libs `pkg-config` and `libudev-dev` are required (pulled in transitively
  by `serialport` / `nvml-wrapper`). They are preinstalled in this environment.

### Running the app
- Run with `cargo run --bin thalamic-relay` (or `./target/debug/thalamic-relay`).
  It is a long-running foreground loop with no graceful arg parsing: despite the
  `clap` dependency, `main` never parses args, so `--help` does NOT print help —
  it just starts the supervisor. Run it in tmux / background when testing.
- It degrades gracefully with no GPU/FPGA: prints `nvidia-smi hung` / runs in
  "software-only mode" and keeps stepping the in-process spiking network.
- Single-instance guard: writes `/tmp/thalamic_relay.lock` with its PID. A stale
  lock for a dead PID is ignored automatically, but a second concurrent instance
  exits immediately. Delete the lockfile only if no instance is actually running.

### Interfaces (used for end-to-end testing)
- UDP IPC on `127.0.0.1:9898`: send JSON `{"type":"Stimuli","values":[...16 f32]}`,
  `{"type":"LearningReward","dopamine_delta":..,"cortisol_delta":..}`, or
  `{"type":"GetNeuroState"}` (which replies with a JSON neuromodulator/spike state).
- Prometheus metrics on `http://localhost:9000/metrics`.
- Both bind on startup, so only one instance can run at a time.
