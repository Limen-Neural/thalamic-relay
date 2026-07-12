use clap::Parser;
use neuromod::{NeuroModulators, SpikingNetwork};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thalamic_relay::cpu::{self, RelayMetrics};
use thalamic_relay::gpu::{GpuTelemetry, HardwareBridge, SafetyStatus};
use tokio::task::JoinHandle;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    struct LockGuard(String);
    impl Drop for LockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // Acquire the single-instance lock atomically BEFORE binding any ports.
    // This ensures the clean "Another instance is already active" message
    // appears instead of a Prometheus bind panic when two instances race.
    let lock_path = "/tmp/thalamic_relay.lock";
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

    let metrics_addr = std::net::SocketAddr::new(cli.metrics_ip, 9000);
    cpu::init_telemetry(metrics_addr);

    let relay_metrics = Arc::new(Mutex::new(RelayMetrics::default()));
    let metrics_clone = Arc::clone(&relay_metrics);
    tokio::spawn(async move {
        cpu::run_metrics_collector(metrics_clone).await;
    });

    println!("[relay] --- Thalamic Relay ---");
    if cli.force_software_only {
        println!("[relay] running in software-only mode (forced via --force-software-only)");
    } else {
        println!("[relay] running in software-only mode (no FPGA/silicon-bridge)");
    }

    let mut network = SpikingNetwork::with_dimensions(cli.num_lif, cli.num_izh, cli.num_channels);
    let mut modulators = NeuroModulators::default();
    let mut stimuli = vec![0.0_f32; network.num_channels];
    let mut latest_spike_count: usize = 0;
    let mut step_count: u64 = 0;
    let mut stimuli_applied_count: u64 = 0;
    let mut brake_applied = false;
    let mut ok_count_after_brake: u32 = 0;
    let mut warned_brake_held_sim = false;
    let mut brake_task: Option<JoinHandle<Result<(), String>>> = None;
    let mut release_task: Option<JoinHandle<Result<(), String>>> = None;

    // Detect leftover throttle from a prior crash (hardware PL persists across process restarts).
    // Only seed brake_applied when the current limit matches this relay's expected 50% brake
    // target, so deliberate operator-set sub-default caps are not auto-restored to default.
    if let Some((current_w, default_w, expected_w)) =
        HardwareBridge::power_limit_matches_emergency_brake(0.5)
    {
        eprintln!(
            "[relay] WARNING: GPU power limit {current_w}W matches expected emergency brake \
             target {expected_w}W (default {default_w}W); will auto-release after Ok streak"
        );
        brake_applied = true;
    }

    let udp_socket =
        std::net::UdpSocket::bind(cli.udp_addr).expect("FATAL: Failed to bind IPC socket");
    udp_socket
        .set_nonblocking(true)
        .expect("FATAL: Failed to set non-blocking UDP");

    loop {
        step_count += 1;
        let loop_start = Instant::now();
        let telemetry = HardwareBridge::read_telemetry_force(cli.force_software_only);

        let mut ok_count_updated_this_iter = false;
        if brake_task.as_ref().is_some_and(|task| task.is_finished()) {
            let task = brake_task.take().expect("finished brake task exists");
            match task.await {
                Ok(Ok(())) => {
                    brake_applied = true;
                    // The GPU may have already recovered while the brake was being applied.
                    // Re-check current telemetry so a stale brake doesn't linger unnoticed
                    // until the next periodic safety check.
                    let force_software_only = cli.force_software_only;
                    let post_telemetry = tokio::task::spawn_blocking(move || {
                        HardwareBridge::read_telemetry_force(force_software_only)
                    })
                    .await
                    .expect("post-brake telemetry read task panicked");
                    let (post_safety, is_sim) = HardwareBridge::check_safety(&post_telemetry);
                    if matches!(post_safety, SafetyStatus::Ok) && !is_sim {
                        ok_count_after_brake = ok_count_after_brake.saturating_add(1);
                        if ok_count_after_brake >= 3 && release_task.is_none() {
                            release_task = Some(tokio::task::spawn_blocking(|| {
                                HardwareBridge::release_emergency_brake()
                            }));
                        }
                    } else {
                        ok_count_after_brake = 0;
                    }
                    ok_count_updated_this_iter = true;
                }
                Ok(Err(e)) => eprintln!("[relay] Emergency brake failed: {e}"),
                Err(e) => eprintln!("[relay] Brake task panicked: {e}"),
            }
        }
        if release_task.as_ref().is_some_and(|task| task.is_finished()) {
            let task = release_task.take().expect("finished release task exists");
            match task.await {
                Ok(Ok(())) => {
                    ok_count_after_brake = 0;
                    let post_telemetry =
                        HardwareBridge::read_telemetry_force(cli.force_software_only);
                    let (post_safety, is_sim) = HardwareBridge::check_safety(&post_telemetry);
                    // The physical brake has already been released; reflect that in
                    // brake_applied regardless of post-release telemetry so state stays
                    // consistent with reality. Hysteresis (ok_count_after_brake) already
                    // gated the release, so this doesn't weaken safety.
                    brake_applied = false;
                    match post_safety {
                        SafetyStatus::Critical(_) => {
                            eprintln!(
                                "[relay] Safety critical after brake release, re-applying brake"
                            );
                            if brake_task.is_none() {
                                brake_task = Some(tokio::task::spawn_blocking(|| {
                                    HardwareBridge::apply_emergency_brake(0.5)
                                }));
                            }
                        }
                        SafetyStatus::Ok if is_sim => {
                            eprintln!(
                                "[relay] SAFETY: brake released but post-release telemetry is simulated"
                            );
                        }
                        SafetyStatus::Ok | SafetyStatus::Warn(_) => {}
                    }
                }
                Ok(Err(e)) => {
                    ok_count_after_brake = 0;
                    eprintln!("[relay] Brake release failed: {e}");
                }
                Err(e) => {
                    ok_count_after_brake = 0;
                    eprintln!("[relay] Brake release task panicked: {e}");
                }
            }
        }

        // Safety check every 10 steps (rate scales with step_interval_ms)
        if step_count.is_multiple_of(10) {
            let (safety, is_sim) = HardwareBridge::check_safety(&telemetry);
            match safety {
                SafetyStatus::Critical(msg) => {
                    eprintln!("[relay] SAFETY CRITICAL: {msg}");
                    ok_count_after_brake = 0;
                    if !brake_applied && brake_task.is_none() {
                        brake_task = Some(tokio::task::spawn_blocking(|| {
                            HardwareBridge::apply_emergency_brake(0.5)
                        }));
                    }
                }
                SafetyStatus::Warn(msg) => {
                    eprintln!("[relay] SAFETY WARN: {msg}");
                    ok_count_after_brake = 0;
                }
                SafetyStatus::Ok => {
                    if brake_applied {
                        if is_sim {
                            // Hold brake while real telemetry is unavailable; reset hysteresis
                            // so release requires 3 consecutive *real* Ok readings after recovery.
                            ok_count_after_brake = 0;
                            if !warned_brake_held_sim {
                                eprintln!(
                                    "[relay] SAFETY: brake held — telemetry is simulated (no real GPU readings to confirm safe release)"
                                );
                                warned_brake_held_sim = true;
                            }
                        } else if !ok_count_updated_this_iter {
                            warned_brake_held_sim = false;
                            // Require 3 consecutive real Ok safety-check readings before release
                            // (at default 100ms step × every 10 steps ≈ 3s hysteresis).
                            ok_count_after_brake += 1;
                            if ok_count_after_brake >= 3 && release_task.is_none() {
                                release_task = Some(tokio::task::spawn_blocking(|| {
                                    HardwareBridge::release_emergency_brake()
                                }));
                            }
                        }
                    }
                }
            }
        }

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

    /// SNN input channels / stimuli vector size (maps to num_channels in neuromod); must be >= 1
    #[arg(long, default_value_t = 16, env = "THALAMIC_NUM_CHANNELS", value_parser = parse_nonzero_usize)]
    num_channels: usize,

    /// SNN LIF neuron count (maps to num_lif in neuromod); must be >= 1
    #[arg(long, default_value_t = 16, env = "THALAMIC_NUM_LIF", value_parser = parse_nonzero_usize)]
    num_lif: usize,

    /// SNN Izhikevich neuron count (maps to num_izh in neuromod); must be >= 1
    #[arg(long, default_value_t = 5, env = "THALAMIC_NUM_IZH", value_parser = parse_nonzero_usize)]
    num_izh: usize,
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
    use super::*;
    use clap::Parser;
    use neuromod::NeuroModulators;

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
        assert_eq!(cli.num_lif, 16); // default
        assert_eq!(cli.num_izh, 5); // default
    }

    /// Helper: bind a relay socket on a random port (non-blocking) and return
    /// (relay_socket, client_socket, relay_addr).
    fn setup_sockets() -> (
        std::net::UdpSocket,
        std::net::UdpSocket,
        std::net::SocketAddr,
    ) {
        let relay = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        relay.set_nonblocking(true).unwrap();
        let relay_addr = relay.local_addr().unwrap();
        let client = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        (relay, client, relay_addr)
    }

    #[test]
    fn stimuli_populates_and_clamps_values() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        // Send values that should be clamped: 5.0 → 1.0, -3.0 → -1.0, 0.5 → 0.5
        let msg = r#"{"type":"Stimuli","values":[5.0, -3.0, 0.5, 0.1]}"#;
        client.send_to(msg.as_bytes(), relay_addr).unwrap();
        // Small delay so the kernel delivers the datagram
        std::thread::sleep(std::time::Duration::from_millis(5));

        let applied = process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        assert!(applied, "stimuli_applied should be true");
        assert_eq!(stimuli[0], 1.0, "5.0 clamped to 1.0");
        assert_eq!(stimuli[1], -1.0, "-3.0 clamped to -1.0");
        assert_eq!(stimuli[2], 0.5);
        assert_eq!(stimuli[3], 0.1);
    }

    #[test]
    fn stimuli_truncates_when_values_exceed_channels() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 2]; // only 2 channels
        let mut mods = NeuroModulators::default();

        let msg = r#"{"type":"Stimuli","values":[1.0, 2.0, 3.0, 4.0]}"#;
        client.send_to(msg.as_bytes(), relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let applied = process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        assert!(applied);
        assert_eq!(stimuli[0], 1.0);
        assert_eq!(stimuli[1], 1.0); // 2.0 clamped to 1.0
        // Extra values (3.0, 4.0) are ignored; vec stays length 2
    }

    #[test]
    fn learning_reward_applies_positive_deltas() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        let msg = r#"{"type":"LearningReward","dopamine_delta":0.3,"cortisol_delta":0.2}"#;
        client.send_to(msg.as_bytes(), relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        // add_reward/add_stress add to existing (which defaults to some base).
        // We just verify the deltas moved the values upward from default.
        let default_mods = NeuroModulators::default();
        assert!(
            mods.dopamine > default_mods.dopamine,
            "dopamine should increase"
        );
        assert!(
            mods.cortisol > default_mods.cortisol,
            "cortisol should increase"
        );
    }

    #[test]
    fn learning_reward_ignores_negative_deltas() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();
        let before = mods;

        let msg = r#"{"type":"LearningReward","dopamine_delta":-0.5,"cortisol_delta":-0.3}"#;
        client.send_to(msg.as_bytes(), relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        // Negative deltas are clamped to 0 via .max(0.0), so no change
        assert_eq!(mods.dopamine, before.dopamine);
        assert_eq!(mods.cortisol, before.cortisol);
    }

    #[test]
    fn get_neuro_state_responds_with_expected_fields() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        let msg = r#"{"type":"GetNeuroState"}"#;
        client.send_to(msg.as_bytes(), relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        process_udp_messages(&relay, &mut stimuli, &mut mods, 42);

        // Read the response sent back to the client
        let mut resp_buf = [0u8; 4096];
        client.set_nonblocking(true).unwrap();
        // Allow a moment for the response to arrive
        std::thread::sleep(std::time::Duration::from_millis(5));
        let amt = client
            .recv(&mut resp_buf)
            .expect("should receive GetNeuroState response");
        let resp: serde_json::Value = serde_json::from_slice(&resp_buf[..amt]).unwrap();

        assert!(
            resp.get("dopamine").is_some(),
            "response must have 'dopamine'"
        );
        assert!(
            resp.get("cortisol").is_some(),
            "response must have 'cortisol'"
        );
        assert!(
            resp.get("acetylcholine").is_some(),
            "response must have 'acetylcholine'"
        );
        assert!(
            resp.get("lif_spike_count").is_some(),
            "response must have 'lif_spike_count'"
        );
        assert_eq!(resp["lif_spike_count"].as_u64().unwrap(), 42);
    }

    #[test]
    fn invalid_json_does_not_panic() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        // Send malformed JSON
        client.send_to(b"not json at all", relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Should not panic; just silently skip
        let applied = process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        assert!(!applied, "no valid stimuli should be applied");
    }

    #[test]
    fn empty_json_does_not_panic() {
        let (relay, client, relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        // Valid JSON but unknown type
        client.send_to(b"{}", relay_addr).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let applied = process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        assert!(!applied);
    }

    #[test]
    fn empty_udp_no_messages_returns_false() {
        let (relay, _client, _relay_addr) = setup_sockets();
        let mut stimuli = vec![0.0_f32; 4];
        let mut mods = NeuroModulators::default();

        // No messages sent → socket is empty → should return false immediately
        let applied = process_udp_messages(&relay, &mut stimuli, &mut mods, 0);
        assert!(!applied);
    }
}
