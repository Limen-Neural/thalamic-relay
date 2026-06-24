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

/// Spawns a background task to track relay/SNN telemetry.
pub async fn run_metrics_collector() {
    info!("Starting Metrics Collector...");

    // Use StdRng which is Send + Sync (unlike thread_rng)
    let mut rng = StdRng::from_entropy();

    loop {
        // --- 1. Track Online Training Metrics ---
        // Fake training loss for demo/Grafana (real SNN state is reported via UDP GetNeuroState and main loop).
        let training_loss = rng.gen_range(0.1..0.5);
        gauge!("online_training_loss").set(training_loss);

        // Simulate tick rate
        sleep(Duration::from_secs(2)).await;
    }
}
