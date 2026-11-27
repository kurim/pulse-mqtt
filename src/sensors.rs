//! All sensors in one list, used by the publisher and the sensor window.

use crate::{cpu, fans, gpu, storage};
use sysinfo::System;

const GIB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Group {
    Cpu,
    Gpu,
    Memory,
    Drive,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Load,
    Clock,
    Power,
    Temperature,
    Fan,
    Data,
}

pub struct Reading {
    /// Stable identifier and default MQTT key.
    pub id: String,
    pub group: Group,
    pub kind: Kind,
    pub value: Option<f64>,
}

impl Reading {
    pub fn unit(&self) -> &'static str {
        match self.kind {
            Kind::Load => "%",
            Kind::Clock => "MHz",
            Kind::Power => "W",
            Kind::Temperature => "°C",
            // The only fan value given in percent.
            Kind::Fan if self.id.ends_with("_percent") => "%",
            Kind::Fan => "RPM",
            Kind::Data => "GiB",
        }
    }
}

pub struct Sensors {
    system: System,
    gpu: Option<gpu::Gpu>,
    cpu: Option<cpu::AmdCpu>,
    fans: Option<fans::SuperIo>,
}

impl Sensors {
    pub fn open() -> Result<Self, sysinfo::Error> {
        let mut system = System::new()?;
        system.refresh_cpu_usage();
        Ok(Self {
            system,
            gpu: gpu::Gpu::open(),
            cpu: cpu::AmdCpu::open()
                .inspect_err(|e| eprintln!("CPU temperature/power unavailable: {e}"))
                .ok(),
            fans: fans::SuperIo::open()
                .inspect_err(|e| eprintln!("Fan speeds unavailable: {e}"))
                .ok(),
        })
    }

    /// CPU load and power are averages since the previous call.
    pub fn read(&mut self) -> Vec<Reading> {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let fans = self.fans.as_ref().and_then(fans::SuperIo::read);
        let g = self.gpu.as_ref().map(gpu::Gpu::read).unwrap_or_default();

        let reading = |id: &str, group, kind, value| Reading {
            id: id.to_owned(),
            group,
            kind,
            value,
        };
        use Group::{Cpu, Drive, Gpu, Memory};
        use Kind::{Clock, Data, Fan, Load, Power, Temperature};
        let memory = |bytes: u64| Some(bytes as f64 / GIB);
        let load = Some(self.system.global_cpu_usage().into());
        let power = self.cpu.as_mut().and_then(cpu::AmdCpu::power);
        let temperature = self.cpu.as_ref().and_then(cpu::AmdCpu::temperature);
        // shortcut: header 2 = CPU fan, checked against HWiNFO on a ROG STRIX B650E-I only;
        // on other boards pick the right fan_header_N in the sensor window instead.
        let cpu_fan = fans.and_then(|f| f[1]);

        let mut list = vec![
            reading("cpu_load", Cpu, Load, load),
            reading("cpu_power", Cpu, Power, power),
            reading("cpu_temp", Cpu, Temperature, temperature),
            reading("cpu_fan", Cpu, Fan, cpu_fan),
            reading("gpu_load", Gpu, Load, g.load),
            reading("gpu_core_clock", Gpu, Clock, g.clock),
            reading("gpu_power", Gpu, Power, g.power),
            reading("gpu_temp", Gpu, Temperature, g.temp),
            reading("gpu_fan", Gpu, Fan, g.fan_rpm),
            reading("gpu_fan_percent", Gpu, Fan, g.fan_percent),
            reading(
                "memory_available",
                Memory,
                Data,
                memory(self.system.available_memory()),
            ),
            reading(
                "memory_used",
                Memory,
                Data,
                memory(self.system.used_memory()),
            ),
        ];
        for (drive, value) in storage::temperatures() {
            let id = format!("storage_temp_{drive}");
            list.push(reading(&id, Drive, Temperature, Some(value)));
        }
        for (header, rpm) in fans.iter().flatten().enumerate() {
            let id = format!("fan_header_{}", header + 1);
            list.push(reading(&id, Cpu, Fan, *rpm));
        }
        list
    }
}
