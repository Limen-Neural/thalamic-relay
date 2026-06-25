use metrics::{counter, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

/// Shared telemetry state populated by the main loop and UDP handlers
#[derive(Debug, Clone, Default)]
pub struct RelayMetrics {
    pub spike_count: usize,
    pub stimulus_latency_ms: f64,
    pub telemetry_freshness_s: f64,
    pub dopamine: f32,
    pub cortisol: f32,
    pub acetylcholine: f32,
    pub stimuli_applied_count: u64,
}

/// Sets up our logging and metrics engines.
/// If `metrics_addr` is Some (e.g. "0.0.0.0:9090"), configure the HTTP listener
/// via with_http_listener (supported in metrics-exporter-prometheus 0.16+).
/// RUST_LOG (or future log-level arg) still controls tracing via env filter where applicable.
pub fn init_telemetry(metrics_addr: Option<&str>) {
    // 1. Initialize 'tracing' for our structured logs
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    // We use try_set_global_default in case it's already set
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 2. Initialize the Prometheus exporter for our metrics.
    // Defaults to 0.0.0.0:9000 if not provided (preserves prior behavior).
    let effective = metrics_addr.unwrap_or("0.0.0.0:9000");
    let sa: std::net::SocketAddr = effective
        .parse()
        .expect("THALAMIC_METRICS_ADDR must be a valid socket address (ip:port)");

    PrometheusBuilder::new()
        .with_http_listener(sa)
        .install()
        .expect("Failed to install Prometheus recorder");

    info!(
        "Telemetry initialized. Prometheus metrics available on http://{}/metrics",
        effective
    );
}

/// Spawns a background task to track relay/SNN telemetry metrics.
/// Reads from shared state populated by the main loop and UDP handlers.
pub async fn run_metrics_collector(metrics: Arc<Mutex<RelayMetrics>>) {
    info!("Starting Metrics Collector...");

    loop {
        // --- 1. Track SNN / Relay Telemetry Metrics ---
        // Read from shared state populated by the main loop and UDP handlers
        let snapshot = {
            let guard = metrics.lock().unwrap();
            guard.clone()
        };

        // Spike / stimulus metrics (Gauges go up and down)
        gauge!("relay_spike_count").set(snapshot.spike_count as f64);
        gauge!("stimulus_apply_latency_ms").set(snapshot.stimulus_latency_ms);
        gauge!("telemetry_freshness_s").set(snapshot.telemetry_freshness_s);

        // Neuromodulator levels (from main loop + UDP rewards)
        gauge!("modulator_dopamine").set(snapshot.dopamine as f64);
        gauge!("modulator_cortisol").set(snapshot.cortisol as f64);
        gauge!("modulator_acetylcholine").set(snapshot.acetylcholine as f64);

        // Track applied stimuli (Counters only go up)
        counter!("relay_stimuli_applied").absolute(snapshot.stimuli_applied_count);

        // Simulate tick rate
        sleep(Duration::from_secs(2)).await;
    }
}
