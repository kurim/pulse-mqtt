//! AMD Ryzen temperature and package power via the PawnIO module AMDFamily17
//! (families 17h to 1Ah). Register layout as used by LibreHardwareMonitor.

use crate::pawnio::{BusLock, Module};
use std::time::Instant;

const MODULE: &[u8] = include_bytes!("../modules/AMDFamily17.bin");
const SMN_THM_TCON_CUR_TMP: u64 = 0x0005_9800;
const MSR_RAPL_POWER_UNIT: u64 = 0xC001_0299;
const MSR_PKG_ENERGY: u64 = 0xC001_029B;

pub struct AmdCpu {
    module: Module,
    joules_per_count: f64,
    last: (u32, Instant),
}

impl AmdCpu {
    pub fn open() -> Result<Self, String> {
        let module = Module::load(MODULE)?;
        let unit = module
            .read(c"ioctl_read_msr", &[MSR_RAPL_POWER_UNIT])
            .ok_or("reading the energy unit failed")?;
        let energy = energy(&module).ok_or("reading the energy counter failed")?;
        Ok(Self {
            module,
            joules_per_count: joules_per_count(unit),
            last: (energy, Instant::now()),
        })
    }

    /// Tctl/Tdie in °C.
    pub fn temperature(&self) -> Option<f64> {
        let _lock = BusLock::acquire(r"Global\Access_PCI")?;
        let raw = self
            .module
            .read(c"ioctl_read_smn", &[SMN_THM_TCON_CUR_TMP])?;
        Some(decode_temperature(raw as u32))
    }

    /// Average package power in watts since the previous call.
    pub fn power(&mut self) -> Option<f64> {
        let now = (energy(&self.module)?, Instant::now());
        let (before, since) = std::mem::replace(&mut self.last, now);
        // The 32-bit counter wraps; wrapping_sub handles one wrap between two reads.
        let joules = f64::from(now.0.wrapping_sub(before)) * self.joules_per_count;
        let seconds = now.1.duration_since(since).as_secs_f64();
        (seconds > 0.0).then(|| joules / seconds)
    }
}

fn energy(module: &Module) -> Option<u32> {
    module
        .read(c"ioctl_read_msr", &[MSR_PKG_ENERGY])
        .map(|v| v as u32)
}

fn joules_per_count(unit_register: u64) -> f64 {
    0.5f64.powi(((unit_register >> 8) & 0x1F) as i32)
}

fn decode_temperature(raw: u32) -> f64 {
    let celsius = f64::from(raw >> 21) * 0.125;
    // Either flag means the reading uses the range shifted down by 49 °C.
    if raw & 0x8_0000 != 0 || raw & 0x3_0000 == 0x3_0000 {
        celsius - 49.0
    } else {
        celsius
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_applies_range_offset() {
        let raw = 440 << 21; // 55 °C
        assert_eq!(decode_temperature(raw), 55.0);
        assert_eq!(decode_temperature(raw | 0x8_0000), 6.0);
        assert_eq!(decode_temperature(raw | 0x3_0000), 6.0);
        assert_eq!(decode_temperature(raw | 0x1_0000), 55.0);
    }

    #[test]
    fn energy_unit_comes_from_bits_8_to_12() {
        assert_eq!(joules_per_count(0x000A_1003), 1.0 / 65536.0);
    }
}
