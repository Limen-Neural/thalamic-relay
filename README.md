# Thalamic Relay

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/rmems/thalamic-relay#license)

A lightweight CLI relay that observes hardware telemetry and forwards normalized
stimuli to a spiking neural network (software-only; silicon-bridge/**FPGA (Field-Programmable Gate Array)** bridge dep removed for modularity).

## Overview

Thalamic Relay is a Rust-based hardware orchestration relay that provides
real-time monitoring of compute telemetry and drives a spiking neural network
(SNN). It collects GPU/CPU telemetry and steps an in-process SNN, exposing a
control/observability surface over **UDP (User Datagram Protocol)** IPC and Prometheus metrics. The relay is
platform-agnostic: it degrades gracefully to a software-only mode when no GPU
is present.

## Features

- **GPU Telemetry**: Real-time monitoring of GPU sensors via NVML (temperature,
  power, clocks, fan, utilization) with a software fallback
- **Software SNN stepping**: In-process spiking network with neuromodulation (no built-in FPGA bridge dep)
- **Spiking Neural Networks**: In-process SNN stepping via the `neuromod` engine
- **Control IPC**: UDP interface for streaming stimuli, applying reward signals,
  and querying neuromodulator/spike state
- **Metrics Collection**: Prometheus-compatible metrics export
- **Process Safety**: Single-instance protection via a lockfile mechanism

## Installation

### Prerequisites

- Rust 2024 edition (MSRV 1.97.1)
- `pkg-config` (used by some native dependencies)
- Linux operating system (tested on Linux)
- Optional: an NVIDIA GPU with NVML support

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run --bin thalamic-relay
```

## Usage

The relay runs in software-only mode and steps the in-process SNN. Telemetry (GPU/CPU) is collected when available.

While running it exposes two interfaces (addresses configurable via CLI/env; see Configuration):

- **UDP (User Datagram Protocol) IPC** (defaults to 127.0.0.1:9898; newline-free JSON messages):
  - `{"type":"Stimuli","values":[/* up to N f32 (N = --num-channels, default 16) */]}` — drive the network
  - `{"type":"LearningReward","dopamine_delta":<f32>,"cortisol_delta":<f32>}` —
    apply reward/stress modulation
  - `{"type":"GetNeuroState"}` — returns a JSON snapshot of the current
    neuromodulator levels and spike count
- **Prometheus metrics** on `http://localhost:9000/metrics` (bind IP configurable via --metrics-ip)

## Architecture

### Core Modules

- **`gpu`**: Hardware bridge for GPU telemetry collection
- **`cpu`**: Telemetry initialization and metrics collection

### Key Components

1. **Hardware Bridge**: Abstract interface for GPU communication
2. **Telemetry System**: Real-time metrics collection and export
3. **SNN Stepping**: In-process spiking neural network execution
4. **Emergency Brakes**: Safety mechanisms for hardware protection

## Dependencies

### Core Dependencies

- `tokio`: Async runtime with full features
- `serde` / `serde_json`: Serialization framework
- `tracing` / `tracing-subscriber`: Structured logging and telemetry
- `metrics` / `metrics-exporter-prometheus`: Metrics collection with Prometheus export
- `neuromod`: Spiking neural network engine

### Hardware Interfaces

- `nvml-wrapper`: GPU monitoring via NVIDIA Management Library

## Configuration

The relay supports CLI flags **and** environment variables (clap derive + "env" feature; added for #11). Defaults preserve prior hardcoded behavior.

Run `thalamic-relay --help` (or `-V`) for the full documented surface.

Key options (with env var equivalent):

- `--udp-addr` / `THALAMIC_UDP_ADDR` (default: 127.0.0.1:9898) — **UDP (User Datagram Protocol)** IPC bind
- `--metrics-ip` / `THALAMIC_METRICS_IP` (default: 127.0.0.1; port is always 9000)
- `--step-interval-ms` / `THALAMIC_STEP_INTERVAL_MS` (default: 100)
- `--force-software-only` / `THALAMIC_FORCE_SOFTWARE_ONLY`
- `--num-channels` / `THALAMIC_NUM_CHANNELS` (input channels/stimuli vector), `--num-lif` / `THALAMIC_NUM_LIF` (LIF neurons), `--num-izh` / `THALAMIC_NUM_IZH` (Izhikevich neurons) — SNN dims for `with_dimensions`
- `RUST_LOG` (standard for tracing; or --log-level in future extensions)

Example with env + flag:
```bash
THALAMIC_UDP_ADDR=0.0.0.0:12345 THALAMIC_METRICS_IP=0.0.0.0 \
  cargo run --bin thalamic-relay -- --force-software-only --step-interval-ms 50
```

## Monitoring

### Prometheus Metrics

The relay exports metrics compatible with Prometheus monitoring, including
GPU telemetry, SNN metrics, and system resource usage.

### Logging

Structured logging via `tracing` with configurable output levels.

## Safety Features

- **Instance Protection**: Lockfile mechanism prevents multiple relay instances (lock acquired before port binding)
- **GPU Safety Monitoring**: Main loop checks thermal (85°C) and power (350W) thresholds every ~1 second
- **Emergency Brakes**: Automatically throttles GPU power limit to 50% via `nvidia-smi -pl` on critical threshold
- **Graceful Degradation**: Continues in software-only mode without GPU; safety checks skip simulated values

## License

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE-2.0](LICENSE-APACHE-2.0) or [http://www.apache.org/licenses/LICENSE-2.0])
- MIT license ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT])

at your option.

## Contributing

Contributions are welcome! Please ensure all submissions follow the project's
coding standards and include appropriate tests.

## Troubleshooting

### Common Issues

1. **Telemetry**: GPU access requires NVML; runs without it in software mode.
2. **GPU Access**: Verify NVML installation and proper permissions
3. **Instance Conflicts**: Check for an existing relay process holding the lockfile
   at `/tmp/thalamic_relay.lock`

### Debug Mode

Enable debug logging for detailed troubleshooting:

```bash
RUST_LOG=debug cargo run --bin thalamic-relay
```
