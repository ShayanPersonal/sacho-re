// All-users autostart management (HKLM registry)
//
// The per-user autostart (HKCU) is handled by tauri-plugin-autostart.
// This module handles the all-users autostart via HKLM, which requires
// admin privileges to write but not to read.

#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, RegDeleteValueW, RegCloseKey,
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_SZ,
};

#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;

use serde::{Deserialize, Serialize};

/// Information about the autostart state for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutostartInfo {
    /// Whether the app was installed for all users (per-machine, in Program Files)
    pub is_per_machine_install: bool,
    /// Whether HKLM autostart is currently enabled (all users)
    pub all_users_autostart: bool,
}

/// Check if the app was installed per-machine (to Program Files)
pub fn is_per_machine_install() -> bool {
    #[cfg(windows)]
    {
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(_) => return false,
        };
        let exe_str = exe.to_string_lossy().to_lowercase();

        // Check against both Program Files paths
        if let Ok(pf) = std::env::var("ProgramFiles") {
            if exe_str.starts_with(&pf.to_lowercase()) {
                return true;
            }
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            if exe_str.starts_with(&pf86.to_lowercase()) {
                return true;
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Check if HKLM autostart entry exists for this app (readable without admin)
pub fn is_hklm_autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        unsafe {
            let subkey = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
            let value_name = to_wide("Sacho");
            let mut hkey: HKEY = std::ptr::null_mut();

            let result = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey.as_ptr(),
                0,
                KEY_READ,
                &mut hkey,
            );

            if result != 0 {
                return false;
            }

            // Check if the value exists by querying its type and size
            let mut value_type: u32 = 0;
            let mut data_size: u32 = 0;
            let exists = RegQueryValueExW(
                hkey,
                value_name.as_ptr(),
                std::ptr::null_mut(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut data_size,
            ) == 0 && value_type == REG_SZ;

            RegCloseKey(hkey);
            exists
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Toggle HKLM autostart by launching self elevated via UAC.
/// Returns Ok(()) if the elevated process was launched successfully.
/// The actual registry write happens in the elevated process (see main.rs).
pub fn request_set_hklm_autostart(enable: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to get current exe path: {}", e))?;

        let verb = to_wide("runas");
        let exe_path = to_wide(&exe.to_string_lossy());
        let args = if enable {
            to_wide("--admin-enable-autostart")
        } else {
            to_wide("--admin-disable-autostart")
        };

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),       // hwnd (null = no parent window)
                verb.as_ptr(),              // lpOperation ("runas" for UAC)
                exe_path.as_ptr(),          // lpFile
                args.as_ptr(),              // lpParameters
                std::ptr::null(),           // lpDirectory
                0,                          // nShowCmd (SW_HIDE)
            )
        };

        // ShellExecuteW returns HINSTANCE > 32 on success
        if result as usize > 32 {
            // Give the elevated process a moment to complete the registry write
            std::thread::sleep(std::time::Duration::from_millis(500));
            Ok(())
        } else {
            Err(format!(
                "Failed to launch elevated process (error code: {}). The user may have cancelled the UAC prompt.",
                result as usize
            ))
        }
    }
    #[cfg(not(windows))]
    {
        Err("All-users autostart is only supported on Windows".to_string())
    }
}

/// Write or remove the HKLM autostart registry entry.
/// This function must be called from an elevated (admin) process.
pub fn write_hklm_autostart(enable: bool) {
    #[cfg(windows)]
    {
        let subkey = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let value_name = to_wide("Sacho");

        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();

            // Open the existing Run key with write access
            let result = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey.as_ptr(),
                0,
                KEY_WRITE,
                &mut hkey,
            );

            if result != 0 {
                eprintln!("Failed to open HKLM Run key for writing (error: {})", result);
                return;
            }

            if enable {
                // Write the autostart entry with the exe path and --autostarted flag
                let exe = match std::env::current_exe() {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("Failed to get exe path: {}", e);
                        RegCloseKey(hkey);
                        return;
                    }
                };
                let value_data = format!("\"{}\" --autostarted", exe.display());
                let value_wide = to_wide(&value_data);
                // Size in bytes, including the null terminator (already in to_wide)
                let byte_size = (value_wide.len() * 2) as u32;

                let write_result = RegSetValueExW(
                    hkey,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    value_wide.as_ptr() as *const u8,
                    byte_size,
                );

                if write_result != 0 {
                    eprintln!("Failed to write HKLM autostart value (error: {})", write_result);
                }
            } else {
                // Remove the autostart entry
                let delete_result = RegDeleteValueW(hkey, value_name.as_ptr());
                if delete_result != 0 && delete_result != 2 {
                    // Error 2 = value not found, which is fine
                    eprintln!("Failed to delete HKLM autostart value (error: {})", delete_result);
                }
            }

            RegCloseKey(hkey);
        }
    }
}

/// Helper: convert a Rust string to a null-terminated UTF-16 Vec for Windows APIs
#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
