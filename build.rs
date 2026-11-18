use std::process::Command;

fn main() {
    if std::env::var_os("ZENKAI_SKIP_SCAN").is_none() {
        // Run host Wi-Fi scanner on every direct cargo build to capture real nearby networks
        let status = Command::new("python3")
            .arg("tools/scan_wifi.py")
            .status();
        if let Err(e) = status {
            eprintln!("cargo:warning=Failed to run Wi-Fi scanner: {}", e);
        }
    }
}
