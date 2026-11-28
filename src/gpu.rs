//! GPU sensors: NVIDIA via NVML, AMD via ADLX (both ship with the driver).

use adlx_lite::{
    AdlxHelper, Gpu as AdlxGpu, GpuMetrics3, Interface, PerformanceMonitoringServices, ffi,
};
use nvml_wrapper::{
    Nvml,
    enum_wrappers::device::{Clock, TemperatureSensor},
};

#[derive(Default)]
pub struct Readings {
    pub load: Option<f64>,
    pub clock: Option<f64>,
    pub power: Option<f64>,
    pub temp: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub fan_percent: Option<f64>,
    pub hotspot: Option<f64>,
    pub memory_clock: Option<f64>,
    /// In MiB.
    pub memory_used: Option<f64>,
    /// In volts.
    pub voltage: Option<f64>,
    pub board_power: Option<f64>,
}

pub enum Gpu {
    Nvidia(Box<Nvml>),
    Amd(Amd),
}

impl Gpu {
    pub fn open() -> Option<Self> {
        // shortcut: first GPU only (NVIDIA wins on mixed systems); add selection for multi-GPU rigs.
        if let Ok(nvml) = Nvml::init()
            && nvml.device_by_index(0).is_ok()
        {
            return Some(Self::Nvidia(Box::new(nvml)));
        }
        Amd::open().map(Self::Amd)
    }

    pub fn read(&self) -> Readings {
        match self {
            Self::Nvidia(nvml) => {
                let Ok(d) = nvml.device_by_index(0) else {
                    return Readings::default();
                };
                Readings {
                    load: d.utilization_rates().ok().map(|u| u.gpu.into()),
                    clock: d.clock_info(Clock::Graphics).ok().map(Into::into),
                    power: d.power_usage().ok().map(|mw| f64::from(mw) / 1000.0),
                    temp: d.temperature(TemperatureSensor::Gpu).ok().map(Into::into),
                    fan_rpm: d.fan_speed_rpm(0).ok().map(Into::into),
                    fan_percent: d.fan_speed(0).ok().map(Into::into),
                    memory_clock: d.clock_info(Clock::Memory).ok().map(Into::into),
                    memory_used: d
                        .memory_info()
                        .ok()
                        .map(|m| m.used as f64 / f64::from(1 << 20)),
                    // NVML does not report these.
                    hotspot: None,
                    voltage: None,
                    board_power: None,
                }
            }
            Self::Amd(amd) => amd.read(),
        }
    }
}

pub struct Amd {
    // Drop order matters: ADLX objects must be released before the helper terminates ADLX.
    gpu: AdlxGpu,
    monitoring: PerformanceMonitoringServices,
    _adlx: AdlxHelper,
}

impl Amd {
    fn open() -> Option<Self> {
        let adlx = AdlxHelper::new().ok()?;
        let monitoring = adlx.system().performance_monitoring_services().ok()?;
        // Prefer the discrete card over the iGPU.
        let gpu = adlx
            .system()
            .gpus()
            .ok()?
            .iter()
            .min_by_key(|g| g.type_().ok() != Some(ffi::ADLX_GPU_TYPE_GPUTYPE_DISCRETE))?;
        Some(Self {
            gpu,
            monitoring,
            _adlx: adlx,
        })
    }

    fn read(&self) -> Readings {
        let Ok(m) = self.monitoring.current_gpu_metrics(&self.gpu) else {
            return Readings::default();
        };
        Readings {
            load: m.usage().ok(),
            clock: m.clock_speed().ok().map(Into::into),
            power: m.power().ok(),
            temp: m.temperature().ok(),
            fan_rpm: m.fan_speed().ok().map(Into::into),
            // Fan percent needs ADLX 1.5; older drivers report n/a.
            fan_percent: m
                .cast::<GpuMetrics3>()
                .and_then(|m| m.fan_duty())
                .ok()
                .map(Into::into),
            hotspot: m.hotspot_temperature().ok(),
            memory_clock: m.vram_clock_speed().ok().map(Into::into),
            memory_used: m.vram().ok().map(Into::into),
            voltage: m.voltage().ok().map(|mv| f64::from(mv) / 1000.0),
            board_power: m.total_board_power().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_reports_temperature() {
        let gpu = Gpu::open().expect("no supported GPU");
        let temp = gpu.read().temp.expect("no temperature");
        assert!((1.0..120.0).contains(&temp), "implausible: {temp}");
    }
}
