//! Drive temperature via IOCTL_STORAGE_QUERY_PROPERTY; works without admin rights.

use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::{
        IO::DeviceIoControl,
        Ioctl::{
            IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_PROPERTY_QUERY,
            STORAGE_TEMPERATURE_DATA_DESCRIPTOR, STORAGE_TEMPERATURE_INFO,
            StorageDeviceTemperatureProperty,
        },
    },
};

/// Descriptor plus room for extra sensors: NVMe drives report several and need the space.
#[repr(C)]
#[derive(Default)]
struct Descriptor {
    head: STORAGE_TEMPERATURE_DATA_DESCRIPTOR,
    more: [STORAGE_TEMPERATURE_INFO; 15],
}

/// (drive name, °C) for every physical drive that reports a temperature.
pub fn temperatures() -> Vec<(String, f64)> {
    // shortcut: probes disk numbers 0..32; use SetupDi enumeration if more are ever needed.
    (0..32)
        .filter_map(|n| Some((format!("disk{n}"), temperature(n)?)))
        .collect()
}

fn temperature(disk: u32) -> Option<f64> {
    let path = device_path(disk);
    // SAFETY: all pointers refer to live locals with matching sizes; the handle is closed below.
    unsafe {
        // Access 0 = metadata only, which is why no elevation is needed.
        let handle = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceTemperatureProperty,
            QueryType: PropertyStandardQuery,
            ..Default::default()
        };
        let mut data = Descriptor::default();
        let mut returned = 0;
        let ok = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            (&raw const query).cast(),
            size_of_val(&query) as u32,
            (&raw mut data).cast(),
            size_of_val(&data) as u32,
            &mut returned,
            null_mut(),
        );
        CloseHandle(handle);

        // The first sensor is the drive's overall temperature.
        let value = data.head.TemperatureInfo[0].Temperature;
        // i16::MIN (0x8000) means "not reported".
        (ok != 0
            && returned as usize >= size_of_val(&data.head)
            && data.head.InfoCount > 0
            && value != i16::MIN)
            .then(|| value.into())
    }
}

/// NUL-terminated UTF-16 device path.
fn device_path(disk: u32) -> Vec<u16> {
    format!(r"\\.\PhysicalDrive{disk}")
        .encode_utf16()
        .chain([0])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_path_is_nul_terminated() {
        let expected: Vec<u16> = "\\\\.\\PhysicalDrive1\0".encode_utf16().collect();
        assert_eq!(device_path(1), expected);
    }
}
