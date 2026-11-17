use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=tools/scan_wifi.py");
    // Run host Wi-Fi scanner to detect real nearby networks
    let status = Command::new("python3")
        .arg("tools/scan_wifi.py")
        .status();
    if let Err(e) = status {
        eprintln!("cargo:warning=Failed to run Wi-Fi scanner: {}", e);
    }
}
