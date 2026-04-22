/// Thin adapter around `silicon-bridge` so this crate does not duplicate UART logic.
pub struct FpgaBridge {
    inner: silicon_bridge::FpgaBridge,
}

impl FpgaBridge {
    /// Keeps compatibility with old call sites; `port_name` is ignored because
    /// silicon-bridge performs dynamic port discovery internally.
    pub fn new(_port_name: &str) -> Result<Self, anyhow::Error> {
        let inner = silicon_bridge::FpgaBridge::new()
            .map_err(|e| anyhow::anyhow!("failed to connect via silicon-bridge: {e}"))?;
        Ok(Self { inner })
    }

    /// Step the FPGA with dynamic normalized stimuli received from upstream encoders.
    pub fn step_cluster(&mut self, stimuli: &[f32]) -> Result<(Vec<f32>, u16), anyhow::Error> {
        let (potentials, spike_flags) = self
            .inner
            .process_stimuli(stimuli)
            .map_err(|e| anyhow::anyhow!("silicon-bridge process_stimuli failed: {e}"))?;

        let spikes = spike_flags
            .iter()
            .take(16)
            .enumerate()
            .fold(0u16, |acc, (idx, spike)| if *spike { acc | (1 << idx) } else { acc });

        Ok((potentials, spikes))
    }
}
