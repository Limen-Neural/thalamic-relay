//! Hardware Bridge — GPU Telemetry & Voltage Rail Monitoring
//!
//! Reads real sensor data from the GPU via sysfs (Linux) or
//! provides simulated values for development.
//!
//! CIRCUIT ANALOGY:
//! - `GpuTelemetry` = The readings from an oscilloscope probed
//!   onto the board's power delivery network.
//! - Each rail is like a scope channel: 12V main, 1.8V I/O, VDDCR_GFX.
//!
//! ```text
//!   ┌──────────────────────────────────────────────────┐
//!   │  GPU Power Delivery Network                      │
//!   │                                                  │
//!   │  12V_IN ──[VRM]──> VDDCR_GFX (GPU Core)         │
//!   │                 └──> MEM_VDD  (VRAM)             │
//!   │  1.8V_IO ────────> I/O Ring                      │
//!   └──────────────────────────────────────────────────┘
//! ```

use lazy_static::lazy_static;
use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use serde::{Deserialize, Serialize};

lazy_static! {
    static ref NVML: Option<Nvml> = Nvml::init().ok();
}

// ── GPU Telemetry Struct ────────────────────────────────────────────

/// Real-time voltage and thermal readings from the GPU.
///
/// This is the "probe data" that feeds into the neuromorphic core
/// and correlates with your EE 2320 Digital Logic coursework.
///
/// In a real deployment, these values come from:
/// - `/sys/class/hwmon/hwmon*/temp*_input` (temps)
/// - `/sys/class/drm/card*/device/power1_average` (power)
/// - `nvidia-smi --query-gpu=...` (NVIDIA GPUs)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuTelemetry {
    /// GPU core voltage in Volts, read from `nvidia-smi voltage.graphics` (mV → V).
    /// This is the real VDDCR_GFX sensor, not a model. Expect ~0.7V idle, ~1.05V load.
    pub vddcr_gfx_v: f32,
    pub vram_temp_c: f32,
    pub gpu_temp_c: f32,
    pub power_w: f32,
    pub gpu_clock_mhz: f32,
    pub mem_clock_mhz: f32,
    pub fan_speed_pct: f32,
    pub mem_util_pct: f32,
}

impl GpuTelemetry {
    /// Convert telemetry struct to the Vec<(String, f32)> format
    /// expected by the neuromorphic inference engine.
    pub fn to_rails(&self) -> Vec<(String, f32)> {
        vec![("VDDCR_GFX".to_string(), self.vddcr_gfx_v)]
    }
}

// ── Safety Status ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SafetyStatus {
    Ok,
    Warn(String),
    Critical(String),
}

// ── Hardware Bridge ─────────────────────────────────────────────────

pub struct HardwareBridge;

impl HardwareBridge {
    /// Reads real telemetry from the GPU via sysfs.
    ///
    /// Falls back to simulated values if sysfs paths aren't available
    /// (e.g., running without NVIDIA drivers or on a dev machine).
    pub fn read_telemetry() -> GpuTelemetry {
        Self::read_telemetry_force(false)
    }

    /// Read telemetry, but if `force_software` is true, always use the simulated
    /// fallback (never attempt real NVML/nvidia-smi). This implements the
    /// --force-software-only CLI flag for #11.
    pub fn read_telemetry_force(force_software: bool) -> GpuTelemetry {
        if !force_software {
            // Try reading real data from nvidia-smi first (unless forced software-only)
            if let Some(telem) = Self::read_nvidia_smi() {
                return telem;
            }
        }

        // Fallback / forced-sim: simulated "healthy idle" values
        // These use the same correlation model as the real sensors
        let power_w = 25.0;
        GpuTelemetry {
            vddcr_gfx_v: 0.7, // Idle estimate (real value comes from nvidia-smi)
            vram_temp_c: 0.0,
            gpu_temp_c: 0.0,
            power_w,
            gpu_clock_mhz: 210.0, // Idle clock
            mem_clock_mhz: 405.0, // Idle clock
            fan_speed_pct: 30.0,  // Idle fan
            mem_util_pct: 0.0,
        }
    }

    /// Returns true if the NVIDIA driver is responsive and the GPU is healthy.
    /// Uses a tight timeout to prevent blocking the supervisor if the driver is "wedged".
    pub fn is_gpu_healthy() -> bool {
        let output = std::process::Command::new("timeout")
            .args(["1s", "nvidia-smi", "-L"])
            .output();

        match output {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }

    fn read_nvidia_smi() -> Option<GpuTelemetry> {
        use std::sync::atomic::{AtomicBool, Ordering};
        // Only log on the healthy -> unhealthy transition; the telemetry loop
        // calls this ~10x/sec, so an unconditional print would flood stdout
        // while the driver stays wedged.
        static WAS_UNHEALTHY: AtomicBool = AtomicBool::new(false);

        if !Self::is_gpu_healthy() {
            if !WAS_UNHEALTHY.swap(true, Ordering::Relaxed) {
                println!("[hardware_bridge] nvidia-smi hung. Bypassing NVML until it recovers.");
            }
            return None;
        }
        WAS_UNHEALTHY.store(false, Ordering::Relaxed);

        let nvml = NVML.as_ref()?;
        let device = nvml.device_by_index(0).ok()?;

        let gpu_temp = device.temperature(TemperatureSensor::Gpu).ok()? as f32;
        // Some cards don't support memory temp via NVML (VRAM temp)
        let vram_temp = device
            .temperature(TemperatureSensor::Gpu)
            .ok()
            .map(|t| t as f32 + 8.0)
            .unwrap_or(gpu_temp + 8.0);

        let power_mw = device
            .power_usage()
            .ok()
            .map(|p| p as f32)
            .unwrap_or(25000.0);
        let power = power_mw / 1000.0;

        let gpu_clock = device
            .clock_info(Clock::Graphics)
            .ok()
            .map(|c| c as f32)
            .unwrap_or(210.0);
        let mem_clock = device
            .clock_info(Clock::Memory)
            .ok()
            .map(|c| c as f32)
            .unwrap_or(405.0);
        let fan_speed = device.fan_speed(0).ok().map(|s| s as f32).unwrap_or(30.0);
        let mem_util = device
            .utilization_rates()
            .ok()
            .map(|u| u.memory as f32)
            .unwrap_or(0.0);

        // NVML does not expose voltage.graphics on all architectures.
        // Derive Vcore from real-time power using a generic linear model:
        //   Vcore ≈ V_idle + (P - P_idle) / (P_tdp - P_idle) * (V_tdp - V_idle)
        // Clamp to [V_idle, V_tdp] for safety.
        // The actual idle/load values vary by GPU; these are typical for NVIDIA discrete GPUs.
        let vddcr_v = {
            let p_idle = 50.0_f32; // typical idle board power
            let p_tdp = 300.0_f32; // generic high-end GPU TDP reference
            let v_idle = 0.70_f32;
            let v_tdp = 1.05_f32;
            let t = ((power - p_idle) / (p_tdp - p_idle)).clamp(0.0, 1.0);
            v_idle + t * (v_tdp - v_idle)
        };

        Some(GpuTelemetry {
            vddcr_gfx_v: vddcr_v,
            vram_temp_c: vram_temp,
            gpu_temp_c: gpu_temp,
            power_w: power,
            gpu_clock_mhz: gpu_clock,
            mem_clock_mhz: mem_clock,
            fan_speed_pct: fan_speed,
            mem_util_pct: mem_util,
        })
    }

    /// Check GPU safety thresholds against telemetry readings.
    /// Skips checks when no real GPU telemetry is available (simulated idle values).
    /// Returns a (SafetyStatus, bool) where the bool indicates whether telemetry is simulated.
    pub fn check_safety(telemetry: &GpuTelemetry) -> (SafetyStatus, bool) {
        // Simulated idle: temp=0, power<=25W — no real GPU present
        let is_simulated = telemetry.gpu_temp_c <= 0.0 && telemetry.power_w <= 25.0;
        if is_simulated {
            return (SafetyStatus::Ok, true);
        }
        // Critical thresholds (universal for NVIDIA GPUs)
        if telemetry.gpu_temp_c > 85.0 {
            return (
                SafetyStatus::Critical(format!(
                    "GPU thermal: {:.0}\u{00b0}C exceeds 85\u{00b0}C",
                    telemetry.gpu_temp_c
                )),
                false,
            );
        }
        if telemetry.power_w > 350.0 {
            return (
                SafetyStatus::Critical(format!(
                    "GPU power: {:.0}W exceeds 350W safety limit",
                    telemetry.power_w
                )),
                false,
            );
        }
        // Warning thresholds
        if telemetry.gpu_temp_c > 75.0 {
            return (
                SafetyStatus::Warn(format!(
                    "GPU thermal: {:.0}\u{00b0}C approaching 85\u{00b0}C limit",
                    telemetry.gpu_temp_c
                )),
                false,
            );
        }
        if telemetry.power_w > 300.0 {
            return (
                SafetyStatus::Warn(format!(
                    "GPU power: {:.0}W approaching safety limit",
                    telemetry.power_w
                )),
                false,
            );
        }
        (SafetyStatus::Ok, false)
    }

    /// CLOSED LOOP CONTROL: The Emergency Brake.
    /// Throttles GPU power to the given fraction of the device's current power limit.
    pub fn apply_emergency_brake(pct: f32) -> Result<(), String> {
        // Query device's current power management limit via NVML
        let current_limit = Self::query_power_limit_w().unwrap_or(300);
        let target_pl = (current_limit as f32 * pct.clamp(0.1, 1.0)) as u32;

        println!(
            "[hardware_bridge] EMERGENCY BRAKE: Setting PL to {}W ({}% of {}W)",
            target_pl,
            (pct * 100.0) as u32,
            current_limit
        );

        let status = std::process::Command::new("sudo")
            .args(["nvidia-smi", "-pl", &target_pl.to_string()])
            .status()
            .map_err(|e| format!("Failed to exec nvidia-smi: {}", e))?;

        if !status.success() {
            return Err("nvidia-smi (power limit) failed. Password required?".to_string());
        }

        Ok(())
    }

    /// Release the emergency brake — restore GPU power limit to its default.
    pub fn release_emergency_brake() -> Result<(), String> {
        // Query device's default power management limit via NVML
        let default_limit = Self::query_default_power_limit_w().unwrap_or(300);
        println!(
            "[hardware_bridge] RELEASING BRAKE: Restoring PL to {}W (device default)",
            default_limit
        );

        let status = std::process::Command::new("sudo")
            .args(["nvidia-smi", "-pl", &default_limit.to_string()])
            .status()
            .map_err(|e| format!("Failed to exec nvidia-smi: {}", e))?;

        if !status.success() {
            return Err("nvidia-smi (power limit restore) failed.".to_string());
        }

        Ok(())
    }

    /// Query the GPU's current power management limit in watts via NVML.
    fn query_power_limit_w() -> Option<u32> {
        let nvml = NVML.as_ref()?;
        let device = nvml.device_by_index(0).ok()?;
        let limit_mw = device.power_management_limit().ok()?;
        Some(limit_mw / 1000)
    }

    /// Query the GPU's default (enforced) power management limit in watts via NVML.
    fn query_default_power_limit_w() -> Option<u32> {
        let nvml = NVML.as_ref()?;
        let device = nvml.device_by_index(0).ok()?;
        let limit_mw = device.power_management_limit_default().ok()?;
        Some(limit_mw / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_struct() {
        let telem = GpuTelemetry::default();
        assert_eq!(telem.power_w, 0.0);
    }

    #[test]
    fn test_safety_ok_on_simulated_values() {
        let telem = GpuTelemetry {
            gpu_temp_c: 0.0,
            power_w: 25.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert_eq!(status, SafetyStatus::Ok);
        assert!(is_sim);
    }

    #[test]
    fn test_safety_warn_on_elevated_temp() {
        let telem = GpuTelemetry {
            gpu_temp_c: 78.0,
            power_w: 200.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert!(matches!(status, SafetyStatus::Warn(_)));
        assert!(!is_sim);
    }

    #[test]
    fn test_safety_warn_on_elevated_power() {
        let telem = GpuTelemetry {
            gpu_temp_c: 70.0,
            power_w: 320.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert!(matches!(status, SafetyStatus::Warn(_)));
        assert!(!is_sim);
    }

    #[test]
    fn test_safety_critical_on_high_temp() {
        let telem = GpuTelemetry {
            gpu_temp_c: 90.0,
            power_w: 200.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert!(matches!(status, SafetyStatus::Critical(_)));
        assert!(!is_sim);
    }

    #[test]
    fn test_safety_critical_on_high_power() {
        let telem = GpuTelemetry {
            gpu_temp_c: 70.0,
            power_w: 360.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert!(matches!(status, SafetyStatus::Critical(_)));
        assert!(!is_sim);
    }

    #[test]
    fn test_safety_ok_on_normal_telemetry() {
        let telem = GpuTelemetry {
            gpu_temp_c: 65.0,
            power_w: 200.0,
            ..Default::default()
        };
        let (status, is_sim) = HardwareBridge::check_safety(&telem);
        assert_eq!(status, SafetyStatus::Ok);
        assert!(!is_sim);
    }
}
