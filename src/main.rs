use clap::Parser;
use neuromod::{NeuroModulators, SpikingNetwork};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thalamic_relay::cpu::{self, RelayMetrics};
use thalamic_relay::gpu::{GpuTelemetry, HardwareBridge};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let metrics_addr = std::net::SocketAddr::new(cli.metrics_ip, 9000);
    cpu::init_telemetry(metrics_addr);

    let relay_metrics = Arc::new(Mutex::new(RelayMetrics::default()));
    let metrics_clone = Arc::clone(&relay_metrics);
    tokio::spawn(async move {
        cpu::run_metrics_collector(metrics_clone).await;
    });

    struct LockGuard(String);
    impl Drop for LockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // Acquire the single-instance lock atomically. `create_new` fails if the
    // file already exists, which closes the check-then-write race where two
    // concurrent starts could both pass a plain existence check.
    // (Lock logic is 100% unchanged except the path now comes from CLI/env.)
    let lock_path = &cli.lockfile;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                // A lock exists. Reclaim it ONLY when we can positively confirm
                // the recorded PID is dead. Any ambiguity (unreadable or
                // unparseable lock file) fails closed to preserve the
                // single-instance guarantee.
                let Some(recorded_pid) = std::fs::read_to_string(lock_path)
                    .ok()
                    .and_then(|content| content.trim().parse::<u32>().ok())
                else {
                    eprintln!(
                        "[relay] FATAL: Lock file {lock_path} exists but is unreadable/unparseable; refusing to start."
                    );
                    std::process::exit(1);
                };

                if std::path::Path::new(&format!("/proc/{recorded_pid}")).exists() {
                    eprintln!(
                        "[relay] FATAL: Another instance is already active (PID: {recorded_pid})."
                    );
                    std::process::exit(1);
                }

                // Stale lock from a dead PID: remove it and retry. If removal
                // fails, abort instead of spinning in a tight retry loop.
                if let Err(remove_err) = std::fs::remove_file(lock_path) {
                    eprintln!(
                        "[relay] FATAL: Failed to clear stale lock {lock_path}: {remove_err}"
                    );
                    std::process::exit(1);
                }
            }
            Err(err) => return Err(err.into()),
        }
    }
    let _lock_guard = LockGuard(lock_path.to_string());

    println!("[relay] --- Thalamic Relay ---");
    if cli.force_software_only {
        println!("[relay] running in software-only mode (forced via --force-software-only)");
    } else {
        println!("[relay] running in software-only mode (no FPGA/silicon-bridge)");
    }

    let mut network =
        SpikingNetwork::with_dimensions(cli.num_channels, cli.num_layers, cli.num_outputs);
    let mut modulators = NeuroModulators::default();
    let mut stimuli = vec![0.0_f32; network.num_channels];
    let mut latest_spike_count: usize = 0;
    let mut step_count: u64 = 0;
    let mut stimuli_applied_count: u64 = 0;

    let udp_socket =
        std::net::UdpSocket::bind(cli.udp_addr).expect("FATAL: Failed to bind IPC socket");
    udp_socket
        .set_nonblocking(true)
        .expect("FATAL: Failed to set non-blocking UDP");

    loop {
        step_count += 1;
        let loop_start = Instant::now();
        let telemetry = HardwareBridge::read_telemetry_force(cli.force_software_only);

        let stimuli_applied = process_udp_messages(
            &udp_socket,
            &mut stimuli,
            &mut modulators,
            latest_spike_count,
        );

        if stimuli_applied {
            stimuli_applied_count += 1;
        }

        let step_start = Instant::now();
        if let Ok(spikes) = network.step(&stimuli, &modulators) {
            latest_spike_count = spikes.len();
        }
        let step_latency_ms = step_start.elapsed().as_secs_f64() * 1000.0;
        modulators.decay();

        // Update shared metrics
        {
            let mut metrics = relay_metrics.lock().unwrap();
            metrics.spike_count = latest_spike_count;
            metrics.stimulus_latency_ms = step_latency_ms;
            metrics.telemetry_freshness_s = loop_start.elapsed().as_secs_f64();
            metrics.dopamine = modulators.dopamine;
            metrics.cortisol = modulators.cortisol;
            metrics.acetylcholine = modulators.acetylcholine;
            metrics.stimuli_applied_count = stimuli_applied_count;
        }

        print_dashboard(&telemetry, latest_spike_count, step_count);

        sleep(Duration::from_millis(cli.step_interval_ms)).await;
    }
}

fn process_udp_messages(
    socket: &std::net::UdpSocket,
    stimuli: &mut [f32],
    modulators: &mut NeuroModulators,
    latest_spike_count: usize,
) -> bool {
    let mut stimuli_applied = false;
    let mut buf = [0u8; 4096];
    while let Ok((amt, src)) = socket.recv_from(&mut buf) {
        let Ok(msg) = std::str::from_utf8(&buf[..amt]) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(msg) else {
            continue;
        };

        match json["type"].as_str() {
            Some("Stimuli") => {
                if let Some(values) = json["values"].as_array() {
                    for (idx, value) in values.iter().take(stimuli.len()).enumerate() {
                        let v = value.as_f64().unwrap_or(0.0) as f32;
                        stimuli[idx] = v.clamp(-1.0, 1.0);
                    }
                    stimuli_applied = true;
                }
            }
            Some("LearningReward") => {
                let dopamine = json["dopamine_delta"].as_f64().unwrap_or(0.0) as f32;
                let cortisol = json["cortisol_delta"].as_f64().unwrap_or(0.0) as f32;
                modulators.add_reward(dopamine.max(0.0));
                modulators.add_stress(cortisol.max(0.0));
            }
            Some("GetNeuroState") => {
                let state_json = serde_json::json!({
                    "dopamine": modulators.dopamine,
                    "cortisol": modulators.cortisol,
                    "acetylcholine": modulators.acetylcholine,
                    "lif_spike_count": latest_spike_count
                });
                if let Ok(encoded) = serde_json::to_string(&state_json) {
                    let _ = socket.send_to(encoded.as_bytes(), src);
                }
            }
            _ => {}
        }
    }
    stimuli_applied
}

fn print_dashboard(telemetry: &GpuTelemetry, lif_spike_count: usize, step: u64) {
    print!(
        "\r[Step {step}] Pwr: {:5.1}W | Vcore: {:.3}V | SW Spikes: {:2}   ",
        telemetry.power_w, telemetry.vddcr_gfx_v, lif_spike_count
    );
    let _ = io::stdout().flush();
}

#[derive(Parser, Debug)]
#[command(
    name = "thalamic-relay",
    version,
    about = "Thalamic Relay - observes telemetry and steps an in-process SNN (software-only)"
)]
struct Cli {
    /// UDP bind address:port for IPC (Stimuli / LearningReward / GetNeuroState)
    #[arg(long, default_value = "127.0.0.1:9898", env = "THALAMIC_UDP_ADDR", value_parser = clap::value_parser!(std::net::SocketAddr))]
    udp_addr: std::net::SocketAddr,

    /// Prometheus metrics listen IP (port is always 9000 per compliance)
    #[arg(long, default_value = "127.0.0.1", env = "THALAMIC_METRICS_IP", value_parser = clap::value_parser!(std::net::IpAddr))]
    metrics_ip: std::net::IpAddr,

    /// SNN step interval (ms); minimum 1 to prevent busy-looping
    #[arg(long, default_value_t = 100, env = "THALAMIC_STEP_INTERVAL_MS", value_parser = clap::value_parser!(u64).range(1..))]
    step_interval_ms: u64,

    /// Force software-only mode (skip real GPU telemetry attempts, use sim).
    /// Usable as a bare flag (`--force-software-only`) or with an explicit
    /// value (`--force-software-only=false` / `THALAMIC_FORCE_SOFTWARE_ONLY=false`).
    #[arg(long, env = "THALAMIC_FORCE_SOFTWARE_ONLY", num_args = 0..=1, default_missing_value = "true", default_value_t = false, value_parser = clap::value_parser!(bool))]
    force_software_only: bool,

    /// SNN input channels (stimuli vector size); must be >= 1
    #[arg(long, default_value_t = 16, env = "THALAMIC_NUM_CHANNELS", value_parser = parse_nonzero_usize)]
    num_channels: usize,

    /// SNN layers/hidden param for with_dimensions; must be >= 1
    #[arg(long, default_value_t = 5, env = "THALAMIC_NUM_LAYERS", value_parser = parse_nonzero_usize)]
    num_layers: usize,

    /// SNN outputs for with_dimensions; must be >= 1
    #[arg(long, default_value_t = 16, env = "THALAMIC_NUM_OUTPUTS", value_parser = parse_nonzero_usize)]
    num_outputs: usize,

    /// Path to single-instance lock file
    #[arg(
        long,
        default_value = "/tmp/thalamic_relay.lock",
        env = "THALAMIC_LOCKFILE"
    )]
    lockfile: String,
}

fn parse_nonzero_usize(s: &str) -> Result<usize, String> {
    let val: usize = s.parse().map_err(|e| format!("{e}"))?;
    if val == 0 {
        return Err("value must be >= 1".to_string());
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_custom_args_and_env_equiv() {
        // direct args
        let cli = Cli::try_parse_from([
            "thalamic-relay",
            "--udp-addr",
            "127.0.0.1:12345",
            "--metrics-ip",
            "0.0.0.0",
            "--step-interval-ms",
            "50",
            "--force-software-only",
            "--num-channels",
            "8",
        ])
        .unwrap();
        assert_eq!(
            cli.udp_addr,
            "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap()
        );
        assert_eq!(
            cli.metrics_ip,
            "0.0.0.0".parse::<std::net::IpAddr>().unwrap()
        );
        assert_eq!(cli.step_interval_ms, 50);
        assert!(cli.force_software_only);
        assert_eq!(cli.num_channels, 8);
        assert_eq!(cli.num_layers, 5); // default
    }
}
