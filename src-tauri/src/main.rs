// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Handle elevated admin autostart commands (UAC-triggered, short-lived)
    // These are launched by the app itself via ShellExecuteW("runas") to modify
    // HKLM registry entries. They run elevated, do one registry write, and exit.
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--admin-enable-autostart") {
        sacho_lib::autostart::write_hklm_autostart(true);
        return;
    }
    if args.iter().any(|a| a == "--admin-disable-autostart") {
        sacho_lib::autostart::write_hklm_autostart(false);
        return;
    }

    sacho_lib::run()
}
