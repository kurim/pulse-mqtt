//! Motherboard fan speeds from a Nuvoton NCT6799D Super I/O chip via the PawnIO module
//! LpcIO. Access sequence as used by LibreHardwareMonitor.

use crate::pawnio::{BusLock, Module};
use std::{thread, time::Duration};

const MODULE: &[u8] = include_bytes!("../modules/LpcIO.bin");
const ISA_BUS: &str = r"Global\Access_ISABUS.HTP.Method";
/// Fan tachometer registers (high byte; the low byte follows).
const TACH: [u16; 7] = [0x4B0, 0x4B2, 0x4B4, 0x4B6, 0x4B8, 0x4BA, 0x4CC];

pub struct SuperIo {
    module: Module,
    slot: u64,
    base: u16,
}

impl SuperIo {
    // shortcut: NCT6799D only; add chip IDs and their register tables for other boards.
    pub fn open() -> Result<Self, String> {
        let module = Module::load(MODULE)?;
        let _lock = BusLock::acquire(ISA_BUS).ok_or("ISA bus is busy")?;
        let (slot, base) = [0, 1]
            .into_iter()
            .find_map(|slot| Some((slot, probe(&module, slot)?)))
            .ok_or("no Nuvoton NCT6799D found")?;
        Ok(Self { module, slot, base })
    }

    /// RPM per fan header; `None` where the chip gives no valid value.
    pub fn read(&self) -> Option<[Option<f64>; TACH.len()]> {
        let _lock = BusLock::acquire(ISA_BUS)?;
        // Vendor ID 0x5CA3 proves the registers are reachable. The firmware can re-lock the
        // I/O space at any time, so unlock again if it does not read back.
        if self.vendor_id() != Some(0x5CA3) {
            probe(&self.module, self.slot)?;
            if self.vendor_id() != Some(0x5CA3) {
                return None;
            }
        }
        Some(TACH.map(|register| rpm(self.byte(register)?, self.byte(register + 1)?)))
    }

    fn vendor_id(&self) -> Option<u16> {
        Some(u16::from_be_bytes([self.byte(0x804F)?, self.byte(0x004F)?]))
    }

    /// Reads one banked hardware monitor register (bank in the high byte).
    fn byte(&self, register: u16) -> Option<u8> {
        let [bank, index] = register.to_be_bytes();
        let (address, data) = (self.base + 5, self.base + 6);
        outb(&self.module, address, 0x4E)?; // bank select
        outb(&self.module, data, bank)?;
        outb(&self.module, address, index)?;
        self.module
            .read(c"ioctl_pio_inb", &[data.into()])
            .map(|v| v as u8)
    }
}

fn outb(module: &Module, port: u16, value: u8) -> Option<()> {
    module.call(c"ioctl_pio_outb", &[port.into(), value.into()], &mut [])
}

/// Returns the hardware monitor base address if the chip in this slot is supported.
/// The caller holds the ISA bus lock.
fn probe(module: &Module, slot: u64) -> Option<u16> {
    let port = if slot == 0 { 0x2E } else { 0x4E };
    module.call(c"ioctl_select_slot", &[slot], &mut [])?;
    // Enter configuration mode, and always leave it again.
    outb(module, port, 0x87)?;
    outb(module, port, 0x87)?;
    let base = configure(module);
    outb(module, port, 0xAA)?;
    base
}

fn configure(module: &Module) -> Option<u16> {
    let read = |register: u64| module.read(c"ioctl_superio_inb", &[register]);
    let write =
        |register: u64, value: u64| module.call(c"ioctl_superio_outb", &[register, value], &mut []);

    if (read(0x20)?, read(0x21)?) != (0xD8, 0x02) {
        return None;
    }
    // Tells the module which I/O ports it may allow.
    module.call(c"ioctl_find_bars", &[], &mut [])?;
    write(0x07, 0x0B)?; // logical device: hardware monitor
    let word = || module.read(c"ioctl_superio_inw", &[0x60]);
    let first = word()?;
    thread::sleep(Duration::from_millis(1));
    if word()? != first {
        return None;
    }
    let options = read(0x28)?;
    if options & 0x10 != 0 {
        write(0x28, options & !0x10)?; // lift the I/O space lock
    }
    base_address(first as u16)
}

fn base_address(raw: u16) -> Option<u16> {
    let address = if raw & 0x07 == 0x05 {
        raw & 0xFFF8
    } else {
        raw
    };
    (address >= 0x100 && address & 0xF007 == 0).then_some(address)
}

fn rpm(high: u8, low: u8) -> Option<f64> {
    let count = u32::from(high) << 5 | u32::from(low & 0x1F);
    match count {
        0x1FFF.. => Some(0.0), // stopped
        0x15.. => Some(1.35e6 / f64::from(count)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpm_from_13_bit_count() {
        assert_eq!(rpm(0xFF, 0x1F), Some(0.0));
        assert_eq!(rpm(0x2A, 0x06), Some(1.35e6 / 1350.0));
        assert_eq!(rpm(0x00, 0x00), None);
    }

    #[test]
    fn base_address_is_validated() {
        assert_eq!(base_address(0x290), Some(0x290));
        assert_eq!(base_address(0x295), Some(0x290));
        assert_eq!(base_address(0xFFFF), None);
        assert_eq!(base_address(0), None);
    }
}
