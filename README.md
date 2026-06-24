# Thalamic Relay

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/Limen-Neural/thalamic-relay#license)

A lightweight CLI relay that observes hardware telemetry and forwards normalized
stimuli to a spiking neural network (software-only; silicon-bridge/**FPGA (Field-Programmable Gate Array)** bridge dep removed for modularity).

## Overview

Thalamic Relay is a Rust-based software relay that provides
real-time monitoring of compute telemetry and drives a spiking neural network
(SNN) in software. It collects GPU/CPU telemetry and exposes a control/observability surface over
UDP IPC and Prometheus metrics. The relay is platform-agnostic (hardware bridge support removed for modularity).

## Features

- **GPU Telemetry**: Real-time monitoring of GPU sensors via NVML (temperature,
  power, clocks, fan, utilization) with a software fallback
- **Software SNN stepping**: In-process spiking network with neuromodulation (no built-in hardware bridge deps)
- **Spiking Neural Networks**: In-process SNN stepping via the `neuromod` engine (software-only)
- **Control IPC**: UDP interface for streaming stimuli, applying reward signals,
  and querying neuromodulator/spike state
- **Metrics Collection**: Prometheus-compatible metrics export
- **Process Safety**: Single-instance protection via a lockfile mechanism

## Installation

### Prerequisites

- Rust 2024 edition (toolchain >= 1.85)
- `pkg-config` development headers
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

The relay runs in software-only mode (FPGA/silicon-bridge backend support removed for modularity) and steps the in-process SNN. Telemetry (GPU/CPU) is still collected.

While running it exposes two interfaces:

- **UDP IPC** on `127.0.0.1:9898` (newline-free JSON messages):
  - `{"type":"Stimuli","values":[/* up to 16 f32 */]}` — drive the network
  - `{"type":"LearningReward","dopamine_delta":<f32>,"cortisol_delta":<f32>}` —
    apply reward/stress modulation
  - `{"type":"GetNeuroState"}` — returns a JSON snapshot of the current
    neuromodulator levels and spike count
- **Prometheus metrics** on `http://localhost:9000/metrics`

## Architecture

### Core Modules

- **`gpu`**: Hardware bridge for GPU telemetry collection
- **`cpu`**: Telemetry initialization and metrics collection
- **`models`**: Shared data models for hardware components

### Key Components

1. **Telemetry System**: Real-time metrics collection and export
2. **Inference Loop**: Steps the SNN and forwards normalized stimuli
3. **Emergency Brakes**: Safety mechanisms for hardware protection (degraded)

## Dependencies

### Core Dependencies

- `tokio`: Async runtime with full features
- `serde` / `serde_json`: Serialization framework
- `tracing` / `tracing-subscriber`: Structured logging and telemetry
- `metrics` / `metrics-exporter-prometheus`: Metrics collection with Prometheus export
- `anyhow`: Error handling
- `neuromod`: Spiking neural network engine

### Hardware Interfaces

- `nvml-wrapper`: GPU monitoring via NVIDIA Management Library
- `nix`: System interfaces for signal handling

## Configuration

The relay uses environment-based configuration. Key areas include:

- GPU monitoring parameters
- Metrics export endpoints
- Logging levels and outputs

## Monitoring

### Prometheus Metrics

The relay exports metrics compatible with Prometheus monitoring, including
GPU/CPU telemetry, training loss (demo), and system resource usage.

### Logging

Structured logging via `tracing` with configurable output levels.

## Safety Features

- **Instance Protection**: Lockfile mechanism prevents multiple relay instances
- **Graceful Degradation**: Runs in software-only mode (hardware bridge support removed)

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
