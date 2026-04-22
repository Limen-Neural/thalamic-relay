pub use plasticity_lab::{
    SpikenautTrainer,
    TrainerError,
    TrainingConfig,
    TrainingExample,
    TrainingSummary,
};

/// Adapter helper for GUI/CLI consumers that only need to display training summaries.
pub fn summary_line(summary: &TrainingSummary) -> String {
    format!(
        "steps={} spikes={} avg_reward={:.4}",
        summary.steps_processed, summary.total_spikes, summary.avg_reward
    )
}
