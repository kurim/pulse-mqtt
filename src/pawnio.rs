//! Minimal wrapper around PawnIOLib.dll (installed by PawnIO) and the bus mutexes
//! that monitoring tools share. Opening the driver requires administrator rights.

use libloading::Library;
use std::ffi::{CStr, c_char};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
};

// Fixed path on purpose: an elevated process must not trust %ProgramFiles% from the environment.
// shortcut: assumes the default install location; read it from the registry if that ever differs.
const DLL: &str = r"C:\Program Files\PawnIO\PawnIOLib.dll";

type Open = unsafe extern "system" fn(*mut HANDLE) -> i32;
type Load = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
type Execute = unsafe extern "system" fn(
    HANDLE,
    *const c_char,
    *const u64,
    usize,
    *mut u64,
    usize,
    *mut usize,
) -> i32;
type Close = unsafe extern "system" fn(HANDLE) -> i32;

pub fn installed() -> bool {
    std::path::Path::new(DLL).exists()
}

/// One driver executor with one signed module loaded into it.
pub struct Module {
    handle: HANDLE,
    execute: Execute,
    close: Close,
    _library: Library, // keeps the function pointers valid
}

impl Module {
    pub fn load(blob: &[u8]) -> Result<Self, String> {
        // SAFETY: signatures match PawnIOLib.h; the blob outlives the load call.
        unsafe {
            let library = Library::new(DLL).map_err(|e| format!("{DLL}: {e}"))?;
            let symbol = |e: libloading::Error| e.to_string();
            let open = *library.get::<Open>(b"pawnio_open\0").map_err(symbol)?;
            let load = *library.get::<Load>(b"pawnio_load\0").map_err(symbol)?;
            let execute = *library
                .get::<Execute>(b"pawnio_execute\0")
                .map_err(symbol)?;
            let close = *library.get::<Close>(b"pawnio_close\0").map_err(symbol)?;

            let mut handle = std::ptr::null_mut();
            check("pawnio_open", open(&mut handle))?;
            let module = Self {
                handle,
                execute,
                close,
                _library: library,
            };
            check("pawnio_load", load(handle, blob.as_ptr(), blob.len()))?;
            Ok(module)
        }
    }

    /// Runs a module function; `None` if it fails or returns fewer values than `output` holds.
    pub fn call(&self, name: &CStr, input: &[u64], output: &mut [u64]) -> Option<()> {
        let mut written = 0;
        // SAFETY: pointers and lengths come from live slices.
        let result = unsafe {
            (self.execute)(
                self.handle,
                name.as_ptr(),
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut written,
            )
        };
        (result >= 0 && written >= output.len()).then_some(())
    }

    pub fn read(&self, name: &CStr, input: &[u64]) -> Option<u64> {
        let mut out = [0];
        self.call(name, input, &mut out)?;
        Some(out[0])
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: the handle came from pawnio_open and is closed exactly once.
        unsafe { (self.close)(self.handle) };
    }
}

fn check(what: &str, hresult: i32) -> Result<(), String> {
    match hresult as u32 {
        _ if hresult >= 0 => Ok(()),
        0x8007_0005 => Err(format!("{what}: access denied, run as administrator")),
        code => Err(format!("{what} failed: 0x{code:08X}")),
    }
}

/// Held while talking to a shared bus; released on drop.
pub struct BusLock(HANDLE);

impl BusLock {
    pub fn acquire(name: &str) -> Option<Self> {
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `name` is NUL-terminated; the handle is closed here or in `drop`.
        unsafe {
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if mutex.is_null() {
                return None;
            }
            match WaitForSingleObject(mutex, 100) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Some(Self(mutex)),
                _ => {
                    CloseHandle(mutex);
                    None
                }
            }
        }
    }
}

impl Drop for BusLock {
    fn drop(&mut self) {
        // SAFETY: we own the mutex and the handle.
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
