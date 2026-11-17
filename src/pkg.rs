//! Zenkai OS - In-Built Package Manager (APTITUDE alias APT), Snapstore & Network Manager
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Manages Debian-compatible package state, Snapstore applications, and WiFi/LAN interfaces.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct AptPackage {
    pub name: &'static str,
    pub version: &'static str,
    pub size_kb: usize,
    pub category: &'static str,
    pub description: &'static str,
    pub installed: bool,
}

#[derive(Clone, Debug)]
pub struct SnapApp {
    pub name: &'static str,
    pub version: &'static str,
    pub size_mb: usize,
    pub publisher: &'static str,
    pub description: &'static str,
    pub installed: bool,
}

#[derive(Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: alloc::string::String,
    pub signal_pct: u8,
    pub security: alloc::string::String,
    pub band: alloc::string::String,
    pub connected: bool,
}

pub struct SystemServices {
    pub apt_packages: Vec<AptPackage>,
    pub snap_apps: Vec<SnapApp>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub lan_connected: bool,
    pub lan_ip: &'static str,
    pub wifi_connected: bool,
    pub wifi_ip: alloc::string::String,
    pub active_ssid: alloc::string::String,
    pub custom_ssid: Option<alloc::string::String>,
    pub last_ping_ms: u32,
}

fn load_wifi_from_uefi_fs() -> Option<(Vec<WifiNetwork>, Option<alloc::string::String>)> {
    let path = uefi::CString16::try_from("\\EFI\\BOOT\\wifi_scan.txt").ok()?;
    let fs_proto = uefi::boot::get_image_file_system(uefi::boot::image_handle()).ok()?;
    let mut fs = uefi::fs::FileSystem::new(fs_proto);
    let bytes = fs.read(path.as_ref()).ok()?;
    parse_wifi_scan_txt(&bytes)
}

fn parse_wifi_scan_txt(bytes: &[u8]) -> Option<(Vec<WifiNetwork>, Option<alloc::string::String>)> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut nets = Vec::new();
    let mut detected_ip = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("# IP|") || line.starts_with("# IP:") {
            let ip_part = line[5..].trim();
            if !ip_part.is_empty() {
                detected_ip = Some(alloc::string::String::from(ip_part));
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 4 {
            let ssid = alloc::string::String::from(parts[0].trim());
            let band = alloc::string::String::from(parts[1].trim());
            let security = alloc::string::String::from(parts[2].trim());
            let signal_pct: u8 = parts[3].trim().parse().unwrap_or(75);
            let connected = if parts.len() >= 5 {
                parts[4].trim() == "1" || parts[4].trim().eq_ignore_ascii_case("true")
            } else {
                false
            };
            nets.push(WifiNetwork {
                ssid,
                band,
                security,
                signal_pct,
                connected,
            });
        }
    }
    if nets.is_empty() {
        None
    } else {
        Some((nets, detected_ip))
    }
}

pub fn load_device_wifi() -> (Vec<WifiNetwork>, Option<alloc::string::String>) {
    if let Some(res) = load_wifi_from_uefi_fs() {
        return res;
    }
    (crate::wifi_data::default_scanned_networks(), Some(alloc::string::String::from(crate::wifi_data::default_wifi_ip())))
}

impl SystemServices {
    pub fn new() -> Self {
        let (nets, detected_ip) = load_device_wifi();
        let connected_net = nets.iter().find(|n| n.connected);
        let wifi_connected = connected_net.is_some();
        let active = if let Some(conn) = connected_net {
            conn.ssid.clone()
        } else if let Some(first) = nets.first() {
            first.ssid.clone()
        } else {
            alloc::string::String::from("Disconnected")
        };

        let wifi_ip = if let Some(ip) = detected_ip {
            ip
        } else {
            alloc::string::String::from(crate::wifi_data::default_wifi_ip())
        };

        Self {
            apt_packages: vec![
                AptPackage { name: "aptitude", version: "0.8.13-5", size_kb: 1420, category: "admin", description: "Terminal-based package manager (aliased as apt)", installed: true },
                AptPackage { name: "wpasupplicant", version: "2.10-12", size_kb: 1680, category: "net", description: "WPA/WPA2/WPA3 WiFi authentication daemon", installed: true },
                AptPackage { name: "network-manager", version: "1.42.4", size_kb: 3120, category: "net", description: "Unified WiFi & Ethernet LAN daemon", installed: true },
                AptPackage { name: "snapd", version: "2.59.1", size_kb: 14500, category: "admin", description: "Snap daemon & sandboxed store runtime", installed: true },
                AptPackage { name: "neofetch", version: "7.1.0", size_kb: 340, category: "utils", description: "System info & ASCII logo display tool", installed: false },
                AptPackage { name: "htop", version: "3.2.2", size_kb: 480, category: "utils", description: "Interactive process & memory viewer", installed: false },
                AptPackage { name: "python3-ml", version: "3.11.4", size_kb: 24800, category: "ai/ml", description: "Python runtime with NumPy/Tensor primitives", installed: false },
                AptPackage { name: "tensor-bench", version: "1.4.0", size_kb: 890, category: "ai/ml", description: "R19 matrix & page-fault micro-benchmark", installed: false },
                AptPackage { name: "net-tools", version: "2.10-0", size_kb: 620, category: "net", description: "ifconfig, netstat, route & arp utilities", installed: false },
                AptPackage { name: "curl", version: "7.88.1", size_kb: 510, category: "net", description: "Command-line HTTP/HTTPS transfer tool", installed: false },
                AptPackage { name: "vim-tiny", version: "9.0.13", size_kb: 940, category: "editors", description: "Compact VI Improved text editor", installed: false },
            ],
            snap_apps: vec![
                SnapApp { name: "snap-store", version: "41.3", size_mb: 52, publisher: "Canonical*", description: "Graphical app store for Linux & Zenkai OS", installed: true },
                SnapApp { name: "pytorch-lite", version: "2.1.0", size_mb: 148, publisher: "Meta-AI*", description: "Lightweight tensor inference engine", installed: false },
                SnapApp { name: "vscode-oss", version: "1.84.2", size_mb: 112, publisher: "Microsoft*", description: "Open-source code editor & Rust IDE", installed: false },
                SnapApp { name: "firefox-esr", version: "115.4", size_mb: 86, publisher: "Mozilla*", description: "Privacy-focused web browser", installed: false },
                SnapApp { name: "docker-cli", version: "24.0.7", size_mb: 44, publisher: "Docker*", description: "Container runtime & image builder", installed: false },
                SnapApp { name: "vlc-media", version: "3.0.18", size_mb: 64, publisher: "VideoLAN*", description: "Universal multimedia player", installed: false },
            ],
            wifi_networks: nets,
            lan_connected: true,
            lan_ip: "10.0.2.15/24",
            wifi_connected,
            wifi_ip,
            active_ssid: active,
            custom_ssid: None,
            last_ping_ms: 14,
        }
    }

    pub fn get_active_ssid(&self) -> &str {
        if let Some(custom) = &self.custom_ssid {
            custom.as_str()
        } else {
            &self.active_ssid
        }
    }

    pub fn connect_custom_wifi(&mut self, ssid: &str) {
        self.active_ssid = alloc::string::String::from(ssid);
        self.custom_ssid = Some(alloc::string::String::from(ssid));
        self.wifi_connected = true;
        for net in self.wifi_networks.iter_mut() {
            net.connected = net.ssid == ssid;
        }
    }

    pub fn dpkg_status(&self) -> alloc::string::String {
        use alloc::format;
        let mut out = alloc::string::String::new();
        for pkg in &self.apt_packages {
            if pkg.installed {
                out.push_str(&format!(
                    "Package: {}\nStatus: install ok installed\nPriority: optional\nSection: {}\nInstalled-Size: {}\nMaintainer: Swapnil Ghosh <25btrcl267@jainuniversity.ac.in>\nArchitecture: amd64\nVersion: {}\nDescription: {}\n\n",
                    pkg.name, pkg.category, pkg.size_kb, pkg.version, pkg.description
                ));
            }
        }
        out
    }

    pub fn apt_installed_count(&self) -> usize {
        self.apt_packages.iter().filter(|p| p.installed).count()
    }

    pub fn snap_installed_count(&self) -> usize {
        self.snap_apps.iter().filter(|s| s.installed).count()
    }

    pub fn is_apt_installed(&self, name: &str) -> bool {
        self.apt_packages.iter().any(|p| p.name == name && p.installed)
    }

    pub fn is_snap_installed(&self, name: &str) -> bool {
        self.snap_apps.iter().any(|s| s.name == name && s.installed)
    }

    pub fn toggle_apt(&mut self, idx: usize) -> Option<(&'static str, bool)> {
        if let Some(pkg) = self.apt_packages.get_mut(idx) {
            if pkg.name == "aptitude" {
                return Some((pkg.name, true)); // Protect core aptitude package
            }
            pkg.installed = !pkg.installed;
            Some((pkg.name, pkg.installed))
        } else {
            None
        }
    }

    pub fn install_apt_by_name(&mut self, name: &str) -> Result<usize, &'static str> {
        for pkg in &mut self.apt_packages {
            if pkg.name == name {
                if pkg.installed {
                    return Err("Package is already installed.");
                }
                pkg.installed = true;
                return Ok(pkg.size_kb);
            }
        }
        Err("E: Unable to locate package in zenkai-bookworm repository.")
    }

    pub fn remove_apt_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        if name == "aptitude" {
            return Err("E: Cannot remove essential package 'aptitude'.");
        }
        for pkg in &mut self.apt_packages {
            if pkg.name == name {
                if !pkg.installed {
                    return Err("Package is not installed.");
                }
                pkg.installed = false;
                return Ok(());
            }
        }
        Err("E: Unable to locate package.")
    }

    pub fn toggle_snap(&mut self, idx: usize) -> Option<(&'static str, bool)> {
        if let Some(app) = self.snap_apps.get_mut(idx) {
            if app.name == "snap-store" {
                return Some((app.name, true));
            }
            app.installed = !app.installed;
            Some((app.name, app.installed))
        } else {
            None
        }
    }

    pub fn install_snap_by_name(&mut self, name: &str) -> Result<usize, &'static str> {
        for app in &mut self.snap_apps {
            if app.name == name {
                if app.installed {
                    return Err("snap is already installed.");
                }
                app.installed = true;
                return Ok(app.size_mb);
            }
        }
        Err("error: snap not found in Snapstore catalog.")
    }

    pub fn connect_wifi_idx(&mut self, idx: usize) {
        if idx >= self.wifi_networks.len() {
            return;
        }
        for (i, net) in self.wifi_networks.iter_mut().enumerate() {
            net.connected = i == idx;
        }
        self.wifi_connected = true;
        self.active_ssid = self.wifi_networks[idx].ssid.clone();
        self.custom_ssid = None;
    }

    pub fn refresh_wifi_scan(&mut self) {
        let (mut fresh, detected_ip) = load_device_wifi();
        if fresh.is_empty() {
            fresh = crate::wifi_data::default_scanned_networks();
        }
        if let Some(ip) = detected_ip {
            self.wifi_ip = ip;
        }
        let tsc = crate::timer::read_tsc();
        // Slightly vary signal percentages to reflect real-world live RF fluctuations
        for (i, net) in fresh.iter_mut().enumerate() {
            let delta = ((tsc.wrapping_add(i as u64 * 17) % 7) as i16) - 3;
            let new_sig = (net.signal_pct as i16 + delta).clamp(30, 99) as u8;
            net.signal_pct = new_sig;
            net.connected = self.wifi_connected && net.ssid == self.active_ssid;
        }
        self.wifi_networks = fresh;
    }

    pub fn toggle_lan(&mut self) {
        self.lan_connected = !self.lan_connected;
    }
}

