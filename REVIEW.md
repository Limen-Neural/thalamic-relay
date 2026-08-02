# Thalamic Relay — Review Guide

## Running Tests

```bash
cargo test                    # all tests (19 total)
cargo test --lib              # src/lib.rs tests only (10)
cargo test --bin thalamic-relay  # src/main.rs tests only (9)
cargo test gpu                # gpu safety tests (8)
cargo test cpu                # cpu metrics tests (1)
cargo test test_safety        # safety-related tests
cargo test stimuli            # UDP stimuli tests
cargo test learning_reward    # learning reward tests
```

### Test inventory

| Binary | Test | What it covers |
|--------|------|----------------|
| lib.rs | `test_telemetry_struct` | GpuTelemetry default values |
| lib.rs | `test_safety_ok_on_simulated_values` | Simulated telemetry skips safety |
| lib.rs | `test_safety_warn_on_elevated_temp` | 75-85°C warning threshold |
| lib.rs | `test_safety_warn_on_elevated_power` | 300-350W warning threshold |
| lib.rs | `test_safety_critical_on_high_temp` | >85°C critical threshold |
| lib.rs | `test_safety_critical_on_high_power` | >350W critical threshold |
| lib.rs | `test_safety_critical_on_non_finite_telemetry` | NaN/Inf telemetry handling |
| lib.rs | `test_safety_critical_on_unknown_power_with_real_temperature` | Missing power with real temp |
| lib.rs | `test_safety_ok_on_normal_telemetry` | Normal readings pass |
| lib.rs | `relay_metrics_default_values` | RelayMetrics defaults to 0 |
| main.rs | `parses_custom_args_and_env_equiv` | CLI flag parsing |
| main.rs | `stimuli_populates_and_clamps_values` | Stimuli vector population + clamping |
| main.rs | `stimuli_truncates_when_values_exceed_channels` | Extra values ignored |
| main.rs | `learning_reward_applies_positive_deltas` | Dopamine/cortisol increase |
| main.rs | `learning_reward_ignores_negative_deltas` | Negative deltas clamped to 0 |
| main.rs | `get_neuro_state_responds_with_expected_fields` | GetNeuroState JSON response |
| main.rs | `invalid_json_does_not_panic` | Malformed input handled |
| main.rs | `empty_json_does_not_panic` | Empty/unknown type handled |
| main.rs | `empty_udp_no_messages_returns_false` | No messages → returns false |

## Linting

```bash
cargo clippy --all-targets -- -D warnings   # lint (CI uses --all-features too)
cargo fmt --check                           # format check
cargo fmt                                   # auto-format
```

## Building

```bash
cargo build                  # debug build
cargo build --release        # release build
cargo build --all-features   # CI-equivalent build
```

## Running

```bash
cargo run -- --help                          # show CLI options
cargo run -- --force-software-only           # software-only mode (no GPU)
cargo run -- --udp-addr 127.0.0.1:12345      # custom UDP bind
cargo run -- --step-interval-ms 50           # faster stepping
```

## CI Checks

The CI workflow (`.github/workflows/ci.yml`) pins Rust 1.97.1:

1. `cargo fmt --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo build --all-features`
4. `cargo test --all-features`

Bot reviewers: Codacy, CodeRabbit, Codex, Kilo, Devin, Gitar, Cursor Bugbot.
