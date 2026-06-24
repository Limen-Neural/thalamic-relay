# Thalamic Relay

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)

A lightweight CLI relay that observes hardware telemetry and forwards normalized
stimuli to a spiking neural network and FPGA/network backends.

## Overview

Thalamic Relay is a Rust-based hardware orchestration relay that provides
real-time monitoring of compute telemetry and drives a spiking neural network
(SNN). It collects GPU/CPU telemetry, bridges to FPGA hardware for stimulus
delivery and spike readback, and exposes a control/observability surface over
UDP IPC and Prometheus metrics. The relay is platform-agnostic: it degrades
gracefully to a software-only mode when no GPU or FPGA is present.

## Features

- **GPU Telemetry**: Real-time monitoring of GPU sensors via NVML (temperature,
  power, clocks, fan, utilization) with a software fallback
- **FPGA Bridge**: Stimulus delivery and spike readback through the
  `silicon-bridge` backend
- **Spiking Neural Networks**: In-process SNN stepping via the `neuromod` engine
- **Control IPC**: UDP interface for streaming stimuli, applying reward signals,
  and querying neuromodulator/spike state
- **Metrics Collection**: Prometheus-compatible metrics export
- **Process Safety**: Single-instance protection via a lockfile mechanism

## Installation

### Prerequisites

- Rust 2024 edition (toolchain >= 1.85)
- `pkg-config` and `libudev` development headers (needed by the serial backend)
- Linux operating system (tested on Linux)
- Optional: an NVIDIA GPU with NVML support, and an FPGA device on a serial port

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run --bin thalamic-relay
```

## Usage

The relay automatically attempts to connect to FPGA hardware via the
`silicon-bridge` backend. If no device is found it logs that it is running in
software-only mode and continues stepping the in-process SNN.

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
- **`fpga`**: Thin adapter over the `silicon-bridge` FPGA backend
- **`cpu`**: Telemetry initialization and metrics collection
- **`models`**: Shared data models for hardware components
- **`trainer`**: Re-exports of offline/closed-loop training utilities

### Key Components

1. **Hardware Bridge**: Abstract interface for GPU and FPGA communication
2. **Telemetry System**: Real-time metrics collection and export
3. **Inference Loop**: Steps the SNN and forwards normalized stimuli
4. **Emergency Brakes**: Safety mechanisms for hardware protection

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

### Workspace Backends (local path dependencies)

- `silicon-bridge`: FPGA deployment and UART spike readback

## Configuration

The relay uses environment-based configuration. Key areas include:

- Serial/FPGA backend discovery
- GPU monitoring parameters
- Metrics export endpoints
- Logging levels and outputs

## Monitoring

### Prometheus Metrics

The relay exports metrics compatible with Prometheus monitoring, including
GPU/FPGA telemetry, training loss, and system resource usage.

### Logging

Structured logging via `tracing` with configurable output levels.

## Safety Features

- **Instance Protection**: Lockfile mechanism prevents multiple relay instances
- **Emergency Brakes**: Hardware protection mechanisms
- **Graceful Degradation**: Continues in software-only mode without GPU/FPGA

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

1. **FPGA Detection**: Ensure the FPGA device is connected and accessible to the
   `silicon-bridge` backend
2. **GPU Access**: Verify NVML installation and proper permissions
3. **Instance Conflicts**: Check for an existing relay process holding the lockfile
   at `/tmp/thalamic_relay.lock`

### Debug Mode

Enable debug logging for detailed troubleshooting:

```bash
RUST_LOG=debug cargo run --bin thalamic-relay
```
