# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Documented and single-sourced the MSRV as `rust-version = "1.97.1"` in `Cargo.toml` (#30)

## [0.1.0] - 2026-07-16

### Added

- Initial `thalamic-relay` binary: a Rust CLI that observes hardware telemetry and forwards normalized stimuli to an in-process spiking neural network
- Software-only SNN stepping via `neuromod` with graceful fallback when no GPU is present
- UDP IPC interface for streaming stimuli, applying reward signals, and querying neuromodulator state
- Prometheus-compatible metrics export on `localhost:9000/metrics`
- GPU telemetry collection via NVML (temperature, power, clocks, fan, utilization)
- Hardware safety monitoring with emergency brake, hysteresis recovery, and automatic throttle release
- CLI argument and environment variable parsing using `clap` (derive + env features)
- Single-instance process guard via a PID lockfile
- Initial test suite covering UDP stimuli, learning reward handling, safety thresholds, and metrics defaults
- CI pipeline with formatting, clippy, build, and test checks
- Dual MIT/Apache-2.0 licensing
- README, `AGENTS.md`, and repository `Boundaries` documentation
