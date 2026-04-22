use neuromod::{NeuroModulators, SpikingNetwork};
use std::io::{self, Write};
use std::time::Duration;
use thalamic_relay::cpu;
use thalamic_relay::fpga::FpgaBridge;
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

    let lock_path = "/tmp/thalamic_relay.lock";
    if std::path::Path::new(lock_path).exists() {
        if let Ok(content) = std::fs::read_to_string(lock_path) {
            if let Ok(old_pid) = content.trim().parse::<u32>() {
                if std::path::Path::new(&format!("/proc/{old_pid}")).exists() {
                    eprintln!("[relay] FATAL: Another instance is already active (PID: {old_pid}).");
                    std::process::exit(1);
                }
            }
        }
    }

    std::fs::write(lock_path, std::process::id().to_string())?;
    let _lock_guard = LockGuard(lock_path);

    println!("[relay] --- Thalamic Relay ---");

    let mut bridge = FpgaBridge::new("/dev/ttyUSB0").ok();
    if bridge.is_some() {
        println!("[relay] FPGA connected via silicon-bridge");
    } else {
        println!("[relay] FPGA not detected, running in software-only mode");
    }

    let mut network = SpikingNetwork::with_dimensions(16, 5, 16);
    let mut modulators = NeuroModulators::default();
    let mut stimuli = vec![0.0_f32; network.num_channels];
    let mut latest_spike_count: usize = 0;
    let mut step_count: u64 = 0;

    let udp_socket = std::net::UdpSocket::bind("127.0.0.1:9898")
        .expect("FATAL: Failed to bind IPC socket");
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

        if let Some(ref mut fpga) = bridge {
            match fpga.step_cluster(&stimuli) {
                Ok((potentials, spike_word)) => {
                    print_dashboard(&telemetry, &potentials, spike_word, latest_spike_count, step_count);
                }
                Err(err) => {
                    eprintln!("\n[relay] FPGA sync lost: {err}");
                    bridge = None;
                }
            }
        } else {
            print_dashboard(&telemetry, &[], 0, latest_spike_count, step_count);
        }

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

fn print_dashboard(
    telemetry: &GpuTelemetry,
    potentials: &[f32],
    spikes: u16,
    lif_spike_count: usize,
    step: u64,
) {
    let pot0 = potentials.first().copied().unwrap_or(0.0);
    print!(
        "\r[Step {step}] Pwr: {:5.1}W | Vcore: {:.3}V | FPGA Spikes: {:04X} | SW Spikes: {:2} | Pot0: {:>6.3}   ",
        telemetry.power_w,
        telemetry.vddcr_gfx_v,
        spikes,
        lif_spike_count,
        pot0
    );
    let _ = io::stdout().flush();
}
