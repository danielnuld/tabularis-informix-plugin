//! Windows DLL search-path priming for the Informix ODBC driver.
//!
//! The driver `iclit09b.dll` depends on other CSDK DLLs (`iregt07b.dll`,
//! `igl4n304.dll`, `irclt09b.dll`, ...). When the ODBC Driver Manager loads the
//! driver by its full registered path, Windows resolves those dependencies via
//! the standard search order — which uses the process PATH, **not** the
//! driver's own folder. If the CSDK `bin` is not on PATH the driver fails to
//! load with `IM003` / system error 126 ("the specified module could not be
//! found").
//!
//! To make the plugin resilient to that environment gap, we add the CSDK `bin`
//! to the process DLL search path (`SetDllDirectory`) before connecting,
//! resolving it from the ODBC registry, then `INFORMIXDIR`, then the default
//! install locations.

/// Ensures the CSDK `bin` is on the DLL search path. Runs once per process.
/// No-op on non-Windows. `driver_name` is the registered ODBC driver name.
#[cfg(windows)]
pub fn ensure(driver_name: &str) {
    use std::sync::OnceLock;
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Some(dir) = resolve_csdk_bin(driver_name) {
            set_dll_directory(&dir);
        }
    });
}

#[cfg(not(windows))]
pub fn ensure(_driver_name: &str) {}

#[cfg(windows)]
fn resolve_csdk_bin(driver_name: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // 1) The registered driver path: ODBCINST.INI\<driver>\Driver points at
    //    iclit09b.dll; its directory is the CSDK bin.
    if !driver_name.is_empty() {
        if let Some(dir) = driver_dll_dir(driver_name) {
            if dir.is_dir() {
                return Some(dir);
            }
        }
    }

    // 2) INFORMIXDIR\bin
    if let Ok(idir) = std::env::var("INFORMIXDIR") {
        let p = PathBuf::from(idir).join("bin");
        if p.is_dir() {
            return Some(p);
        }
    }

    // 3) Default install locations.
    for base in [
        r"C:\Program Files (x86)\IBM Informix Client SDK\bin",
        r"C:\Program Files\IBM Informix Client SDK\bin",
    ] {
        let p = PathBuf::from(base);
        if p.is_dir() {
            return Some(p);
        }
    }

    None
}

/// Reads `HKLM\SOFTWARE\ODBC\ODBCINST.INI\<driver>\Driver` and returns the
/// directory of that DLL. A 32-bit process is auto-redirected to WOW6432Node.
#[cfg(windows)]
fn driver_dll_dir(driver_name: &str) -> Option<std::path::PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(format!("SOFTWARE\\ODBC\\ODBCINST.INI\\{driver_name}"))
        .ok()?;
    let driver: String = key.get_value("Driver").ok()?;
    std::path::Path::new(&driver)
        .parent()
        .map(std::path::Path::to_path_buf)
}

/// Adds `dir` to the process DLL search path via `SetDllDirectoryW`.
#[cfg(windows)]
fn set_dll_directory(dir: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn SetDllDirectoryW(path: *const u16) -> i32;
    }

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safety: `wide` is a valid, null-terminated UTF-16 string that outlives
    // the call.
    unsafe {
        SetDllDirectoryW(wide.as_ptr());
    }
}
