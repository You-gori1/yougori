#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    if let Err(error) = yougori_vault::run() {
        eprintln!("Personal Vault: {error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("Personal Vault requires the Windows protected broker on this release.");
        std::process::exit(1);
    }
}
