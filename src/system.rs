//! CPU load, memory and drive usage, straight from Windows.

use std::ptr::null_mut;
use windows_sys::{
    Wdk::System::SystemInformation::{
        NtQuerySystemInformation, SystemProcessorPerformanceInformation,
    },
    Win32::{
        Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives},
        System::{
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            WindowsProgramming::{DRIVE_FIXED, SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION},
        },
    },
};

/// Idle and total time of one logical processor since the start of Windows.
#[derive(Clone, Copy)]
struct Times {
    idle: i64,
    total: i64,
}

fn processor_times() -> Vec<Times> {
    // shortcut: at most 64 logical processors (one processor group); use
    // NtQuerySystemInformationEx per group if a machine has more.
    let mut buffer = [SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION::default(); 64];
    let mut length = 0;
    // SAFETY: the buffer has the size passed in.
    let status = unsafe {
        NtQuerySystemInformation(
            SystemProcessorPerformanceInformation,
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            &mut length,
        )
    };
    if status < 0 {
        return Vec::new();
    }
    let count = length as usize / size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>();
    buffer
        .iter()
        .take(count)
        .map(|p| Times {
            idle: p.IdleTime,
            // The kernel time includes the idle time.
            total: p.KernelTime + p.UserTime,
        })
        .collect()
}

/// Busy share in percent of the time that passed; `None` if none did.
fn percent(idle: i64, total: i64) -> Option<f64> {
    (total > 0).then(|| (100.0 * (total - idle) as f64 / total as f64).clamp(0.0, 100.0))
}

pub struct CpuLoad {
    previous: Vec<Times>,
}

impl CpuLoad {
    pub fn new() -> Self {
        Self {
            previous: processor_times(),
        }
    }

    /// The load of all processors together and of each one, since the previous call.
    pub fn read(&mut self) -> (Option<f64>, Vec<Option<f64>>) {
        let now = processor_times();
        let passed: Vec<(i64, i64)> = now
            .iter()
            .zip(&self.previous)
            .map(|(new, old)| (new.idle - old.idle, new.total - old.total))
            .collect();
        self.previous = now;
        let (idle, total) = passed
            .iter()
            .fold((0, 0), |sum, p| (sum.0 + p.0, sum.1 + p.1));
        let each = passed.iter().map(|&(i, t)| percent(i, t)).collect();
        (percent(idle, total), each)
    }
}

/// Total and available physical memory in bytes.
pub fn memory() -> Option<(u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: the struct carries its own size.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
}

/// Drive letter and used share in percent of every fixed drive. Other drives (removable,
/// optical, network) are not touched.
pub fn fixed_drives() -> Vec<(char, f64)> {
    // SAFETY: no arguments.
    let present = unsafe { GetLogicalDrives() };
    (b'a'..=b'z')
        .enumerate()
        .filter(|(bit, _)| present & (1 << bit) != 0)
        .filter_map(|(_, letter)| {
            let root: [u16; 4] = [letter.into(), b':'.into(), b'\\'.into(), 0];
            let (mut total, mut free) = (0u64, 0u64);
            // SAFETY: a NUL-terminated path and two valid output values.
            let ok = unsafe {
                GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED
                    && GetDiskFreeSpaceExW(root.as_ptr(), null_mut(), &mut total, &mut free) != 0
            };
            // The free space is the "idle" part of the drive.
            let share = percent(i64::try_from(free).ok()?, i64::try_from(total).ok()?);
            ok.then_some((letter.into(), share?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_is_the_busy_share() {
        assert_eq!(percent(75, 100), Some(25.0));
        assert_eq!(percent(0, 0), None);
        // Counters of different precision must not leave the scale.
        assert_eq!(percent(101, 100), Some(0.0));
    }

    #[test]
    fn windows_reports_processors_memory_and_the_system_drive() {
        let mut load = CpuLoad::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (all, each) = load.read();
        let processors = std::thread::available_parallelism().unwrap().get();
        assert_eq!(each.len(), processors.min(64));
        assert!(all.is_some());
        let (total, available) = memory().unwrap();
        assert!(available > 0 && available <= total);
        assert!(fixed_drives().iter().any(|d| d.0 == 'c'));
    }
}
