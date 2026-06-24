use metrics::{counter, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{Level, info, warn};
use tracing_subscriber::FmtSubscriber;

/// Sets up our logging and metrics engines
pub fn init_telemetry() {
    // 1. Initialize 'tracing' for our structured logs
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    // We use try_set_global_default in case it's already set
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 2. Initialize the Prometheus exporter for our metrics
    // This will host a metrics endpoint at http://localhost:9000/metrics
    let builder = PrometheusBuilder::new();
    builder
        .install()
        .expect("Failed to install Prometheus recorder");

    info!("Telemetry initialized. Prometheus metrics available on port 9000.");
}

/// Spawns a background task to track relay/SNN telemetry and FPGA hardware metrics.
pub async fn run_metrics_collector() {
    info!("Starting Metrics Collector...");

    // Use StdRng which is Send + Sync (unlike thread_rng)
    let mut rng = StdRng::from_entropy();

    loop {
        // --- 1. Track SNN / Relay Telemetry Metrics ---
        // Use labels and realistic ranges for Grafana (generalized from prior
        // crypto mining fakes to align with repo boundaries from closed #3 and #9).
        // We use labels to slice data later in dashboards.

        // Spike / stimulus metrics (Gauges go up and down)
        let spike_rate = rng.gen_range(5.0..30.0);
        gauge!("relay_spike_rate").set(spike_rate);
        gauge!("stimulus_apply_latency_ms").set(rng.gen_range(0.5..12.0));
        gauge!("telemetry_freshness_s").set(rng.gen_range(0.05..1.5));

        // Neuromodulator levels (from main loop + UDP rewards)
        gauge!("modulator_dopamine").set(rng.gen_range(0.0..1.0));
        gauge!("modulator_cortisol").set(rng.gen_range(0.0..0.9));
        gauge!("modulator_acetylcholine").set(rng.gen_range(0.0..0.6));

        // Track applied stimuli (Counters only go up)
        if rng.gen_bool(0.08) {
            info!("Relay applied stimulus batch");
            counter!("relay_stimuli_applied").increment(1);
        }

        // --- 2. Track FPGA Hardware Metrics ---
        let fpga_temp = rng.gen_range(60.0..85.0);
        gauge!("fpga_temperature_celsius").set(fpga_temp);

        if fpga_temp > 82.0 {
            warn!("FPGA temperature is getting high: {:.1}°C", fpga_temp);
        }

        // --- 3. Track Online Training Metrics ---
        // As the FPGA learns, we can track its loss or accuracy
        let training_loss = rng.gen_range(0.1..0.5);
        gauge!("online_training_loss").set(training_loss);

        // --- SiliconBridge v3.0: Neuromorphic Risk Metrics ---
        gauge!("silicon_bridge_surprise_max").set(rng.gen_range(0.0..1.0));
        gauge!("silicon_bridge_global_inhibit_status").set(if rng.gen_bool(0.1) {
            1.0
        } else {
            0.0
        });

        // Simulate tick rate
        sleep(Duration::from_secs(2)).await;
    }
}
