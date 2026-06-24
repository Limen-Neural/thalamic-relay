use metrics::{counter, gauge};
use neuromod::{NeuroModulators, SpikingNetwork};
use std::io::{self, Write};
use std::time::Duration;
use thalamic_relay::cpu;
use thalamic_relay::gpu::{GpuTelemetry, HardwareBridge};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    cpu::init_telemetry();
    tokio::spawn(cpu::run_metrics_collector());

    struct LockGuard(&'static str);
    impl Drop for LockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    // Acquire the single-instance lock atomically. `create_new` fails if the
    // file already exists, which closes the check-then-write race where two
    // concurrent starts could both pass a plain existence check.
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
    let _lock_guard = LockGuard(lock_path);

    println!("[relay] --- Thalamic Relay ---");
    println!("[relay] running in software-only mode (no FPGA/silicon-bridge)");

    let mut network = SpikingNetwork::with_dimensions(16, 5, 16);
    let mut modulators = NeuroModulators::default();
    let mut stimuli = vec![0.0_f32; network.num_channels];
    let mut latest_spike_count: usize = 0;
    let mut step_count: u64 = 0;

    let udp_socket =
        std::net::UdpSocket::bind("127.0.0.1:9898").expect("FATAL: Failed to bind IPC socket");
    udp_socket
        .set_nonblocking(true)
        .expect("FATAL: Failed to set non-blocking UDP");

    loop {
        step_count += 1;
        let telemetry = HardwareBridge::read_telemetry();
        process_udp_messages(
            &udp_socket,
            &mut stimuli,
            &mut modulators,
            &network,
            latest_spike_count,
        );

        if let Ok(spikes) = network.step(&stimuli, &modulators) {
            latest_spike_count = spikes.len();
        }
        modulators.decay();

        // Real runtime metrics (not RNG) for observability
        gauge!("relay_spike_count").set(latest_spike_count as f64);
        gauge!("modulator_dopamine").set(modulators.dopamine);
        gauge!("modulator_cortisol").set(modulators.cortisol);
        gauge!("modulator_acetylcholine").set(modulators.acetylcholine);

        print_dashboard(&telemetry, latest_spike_count, step_count);

        sleep(Duration::from_millis(100)).await;
    }
}

fn process_udp_messages(
    socket: &std::net::UdpSocket,
    stimuli: &mut [f32],
    modulators: &mut NeuroModulators,
    network: &SpikingNetwork,
    latest_spike_count: usize,
) {
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
                counter!("relay_stimuli_applied").increment(1);
                if let Some(values) = json["values"].as_array() {
                    for (idx, value) in values.iter().take(stimuli.len()).enumerate() {
                        let v = value.as_f64().unwrap_or(0.0) as f32;
                        stimuli[idx] = v.clamp(-1.0, 1.0);
                    }
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
                    "dopamine": network.modulators.dopamine,
                    "cortisol": network.modulators.cortisol,
                    "acetylcholine": network.modulators.acetylcholine,
                    "lif_spike_count": latest_spike_count
                });
                if let Ok(encoded) = serde_json::to_string(&state_json) {
                    let _ = socket.send_to(encoded.as_bytes(), src);
                }
            }
            _ => {}
        }
    }
}

fn print_dashboard(telemetry: &GpuTelemetry, lif_spike_count: usize, step: u64) {
    print!(
        "\r[Step {step}] Pwr: {:5.1}W | Vcore: {:.3}V | SW Spikes: {:2}   ",
        telemetry.power_w, telemetry.vddcr_gfx_v, lif_spike_count
    );
    let _ = io::stdout().flush();
}
