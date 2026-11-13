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
    pub ssid: &'static str,
    pub signal_pct: u8,
    pub security: &'static str,
    pub band: &'static str,
    pub connected: bool,
}

pub struct SystemServices {
    pub apt_packages: Vec<AptPackage>,
    pub snap_apps: Vec<SnapApp>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub lan_connected: bool,
    pub lan_ip: &'static str,
    pub wifi_connected: bool,
    pub wifi_ip: &'static str,
    pub active_ssid: &'static str,
    pub last_ping_ms: u32,
}

impl SystemServices {
    pub fn new() -> Self {
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
            wifi_networks: vec![
                WifiNetwork { ssid: "Zenkai-Lab-5G", signal_pct: 94, security: "WPA3-SAE", band: "5 GHz", connected: true },
                WifiNetwork { ssid: "Jain-Campus-WiFi", signal_pct: 78, security: "WPA2-EAP", band: "5 GHz", connected: false },
                WifiNetwork { ssid: "Swapnil-iPhone-Hotspot", signal_pct: 86, security: "WPA2-PSK", band: "2.4 GHz", connected: false },
                WifiNetwork { ssid: "Lab-Guest-Open", signal_pct: 52, security: "Open", band: "2.4 GHz", connected: false },
            ],
            lan_connected: true,
            lan_ip: "10.0.2.15/24",
            wifi_connected: true,
            wifi_ip: "192.168.1.104/24",
            active_ssid: "Zenkai-Lab-5G",
            last_ping_ms: 14,
        }
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
        self.active_ssid = self.wifi_networks[idx].ssid;
    }

    pub fn toggle_lan(&mut self) {
        self.lan_connected = !self.lan_connected;
    }
}

