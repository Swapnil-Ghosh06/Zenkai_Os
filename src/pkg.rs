//! Zenkai OS - In-Built Package Manager (APTITUDE alias APT), Snapstore,
//! Ephemeral Role Profiles (.venv) & Dynamic Network Manager
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct AptPackage {
    pub name: &'static str,
    pub version: &'static str,
    pub size_kb: usize,
    pub category: &'static str,
    pub description: &'static str,
    pub cmd_hint: &'static str,
    pub installed: bool,
}

#[derive(Clone, Debug)]
pub struct SnapApp {
    pub name: &'static str,
    pub version: &'static str,
    pub size_mb: usize,
    pub publisher: &'static str,
    pub description: &'static str,
    pub cmd_hint: &'static str,
    pub installed: bool,
}

#[derive(Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal_pct: u8,
    pub security: String,
    pub band: String,
    pub connected: bool,
}

impl WifiNetwork {
    pub fn is_locked(&self) -> bool {
        let s = self.security.trim();
        !s.is_empty() && !s.eq_ignore_ascii_case("open") && !s.eq_ignore_ascii_case("none")
    }
}

#[derive(Clone, Debug)]
pub struct RoleProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub role_badge: &'static str,
    pub venv_name: &'static str,
    pub description: &'static str,
    pub apt_deps: &'static [&'static str],
    pub snap_deps: &'static [&'static str],
    pub pip_modules: &'static [&'static str],
    pub sample_cmd: &'static str,
}

pub struct SystemServices {
    pub apt_packages: Vec<AptPackage>,
    pub snap_apps: Vec<SnapApp>,
    pub profiles: Vec<RoleProfile>,
    pub active_profile: Option<usize>,
    pub profile_temp_apt: Vec<&'static str>,
    pub profile_temp_snap: Vec<&'static str>,
    pub extra_pip_modules: Vec<String>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub lan_connected: bool,
    pub lan_ip: &'static str,
    pub wifi_connected: bool,
    pub wifi_ip: String,
    pub gateway_ip: String,
    pub base_ping_tenths_ms: u32,
    pub active_ssid: String,
    pub custom_ssid: Option<String>,
    pub authenticated_ssids: Vec<(String, String)>,
    pub last_ping_ms: u32,
}

pub struct WifiLoadResult {
    pub networks: Vec<WifiNetwork>,
    pub ip: Option<String>,
    pub gateway: Option<String>,
    pub base_ping_tenths_ms: Option<u32>,
}

fn load_wifi_from_uefi_fs() -> Option<WifiLoadResult> {
    let fs_proto = uefi::boot::get_image_file_system(uefi::boot::image_handle()).ok()?;
    let mut fs = uefi::fs::FileSystem::new(fs_proto);
    for raw_path in ["\\EFI\\BOOT\\wifi_scan.txt", "\\wifi_scan.txt"] {
        if let Ok(path) = uefi::CString16::try_from(raw_path) {
            if let Ok(bytes) = fs.read(path.as_ref()) {
                if let Some(parsed) = parse_wifi_scan_txt(&bytes) {
                    return Some(parsed);
                }
            }
        }
    }
    None
}

fn parse_wifi_scan_txt(bytes: &[u8]) -> Option<WifiLoadResult> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut nets = Vec::new();
    let mut detected_ip = None;
    let mut detected_gw = None;
    let mut detected_ping = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("# IP|") || line.starts_with("# IP:") {
            let ip_part = line[5..].trim();
            if !ip_part.is_empty() {
                detected_ip = Some(String::from(ip_part));
            }
            continue;
        }
        if line.starts_with("# GW|") {
            let mut parts = line[5..].split('|');
            if let Some(gw) = parts.next() {
                let gw = gw.trim();
                if !gw.is_empty() {
                    detected_gw = Some(String::from(gw));
                }
            }
            if let Some(p) = parts.next() {
                if let Ok(v) = p.trim().parse::<u32>() {
                    detected_ping = Some(v);
                }
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 4 {
            let ssid = String::from(parts[0].trim());
            let band = String::from(parts[1].trim());
            let security = String::from(parts[2].trim());
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
    if nets.is_empty() && detected_ip.is_none() {
        None
    } else {
        Some(WifiLoadResult {
            networks: nets,
            ip: detected_ip,
            gateway: detected_gw,
            base_ping_tenths_ms: detected_ping,
        })
    }
}

pub fn load_device_wifi() -> WifiLoadResult {
    if let Some(res) = load_wifi_from_uefi_fs() {
        return res;
    }
    WifiLoadResult {
        networks: crate::wifi_data::default_scanned_networks(),
        ip: Some(String::from(crate::wifi_data::default_wifi_ip())),
        gateway: Some(String::from(crate::wifi_data::default_gateway_ip())),
        base_ping_tenths_ms: Some(crate::wifi_data::default_base_ping_tenths_ms()),
    }
}

impl SystemServices {
    pub fn new() -> Self {
        let loaded = load_device_wifi();
        let nets = loaded.networks;
        let connected_net = nets.iter().find(|n| n.connected);
        let wifi_connected = connected_net.is_some();
        let active = if let Some(conn) = connected_net {
            conn.ssid.clone()
        } else {
            String::from("Disconnected")
        };

        let wifi_ip = loaded
            .ip
            .unwrap_or_else(|| String::from(crate::wifi_data::default_wifi_ip()));
        let gateway_ip = loaded
            .gateway
            .unwrap_or_else(|| String::from(crate::wifi_data::default_gateway_ip()));
        let base_ping_tenths_ms = loaded
            .base_ping_tenths_ms
            .unwrap_or_else(crate::wifi_data::default_base_ping_tenths_ms);

        Self {
            apt_packages: vec![
                AptPackage { name: "aptitude", version: "0.8.13-5", size_kb: 1420, category: "admin", description: "Terminal package manager (aliased as apt)", cmd_hint: "apt list", installed: true },
                AptPackage { name: "wpasupplicant", version: "2.10-12", size_kb: 1680, category: "net", description: "WPA/WPA2/WPA3 WiFi auth daemon", cmd_hint: "wifi scan", installed: true },
                AptPackage { name: "network-manager", version: "1.42.4", size_kb: 3120, category: "net", description: "Unified WiFi & Ethernet LAN daemon", cmd_hint: "ifconfig", installed: true },
                AptPackage { name: "snapd", version: "2.59.1", size_kb: 14500, category: "admin", description: "Snap daemon & sandboxed store runtime", cmd_hint: "snap list", installed: true },
                AptPackage { name: "neofetch", version: "7.1.0", size_kb: 340, category: "utils", description: "System info & ASCII logo display tool", cmd_hint: "neofetch", installed: false },
                AptPackage { name: "htop", version: "3.2.2", size_kb: 480, category: "utils", description: "Interactive process & memory viewer", cmd_hint: "htop", installed: false },
                AptPackage { name: "python3-ml", version: "3.11.4", size_kb: 24800, category: "ai/ml", description: "Python 3.11 + venv & NumPy primitives", cmd_hint: "python3", installed: false },
                AptPackage { name: "tensor-bench", version: "1.4.0", size_kb: 890, category: "ai/ml", description: "R19 matrix & page-fault micro-benchmark", cmd_hint: "tensor-bench", installed: false },
                AptPackage { name: "net-tools", version: "2.10-0", size_kb: 620, category: "net", description: "ifconfig, netstat, route & arp utilities", cmd_hint: "netstat", installed: false },
                AptPackage { name: "curl", version: "7.88.1", size_kb: 510, category: "net", description: "Command-line HTTP/HTTPS transfer tool", cmd_hint: "curl https://zenkai-os.org", installed: false },
                AptPackage { name: "vim-tiny", version: "9.0.13", size_kb: 940, category: "editors", description: "Compact VI Improved text editor", cmd_hint: "vim /etc/apt/sources.list", installed: false },
            ],
            snap_apps: vec![
                SnapApp { name: "snap-store", version: "41.3", size_mb: 52, publisher: "Canonical*", description: "Graphical app store for Linux & Zenkai OS", cmd_hint: "snap list", installed: true },
                SnapApp { name: "pytorch-lite", version: "2.1.0", size_mb: 148, publisher: "Meta-AI*", description: "Lightweight tensor & autograd inference engine", cmd_hint: "pytorch", installed: false },
                SnapApp { name: "vscode-oss", version: "1.88.1", size_mb: 112, publisher: "Microsoft*", description: "VS Code IDE & Terminal Python .venv runner", cmd_hint: "code train_ai.py", installed: false },
                SnapApp { name: "firefox-esr", version: "115.4", size_mb: 86, publisher: "Mozilla*", description: "Privacy-focused web browser & net tester", cmd_hint: "firefox https://zenkai-os.org", installed: false },
                SnapApp { name: "docker-cli", version: "24.0.7", size_mb: 44, publisher: "Docker*", description: "Container runtime & AI image builder", cmd_hint: "docker ps", installed: false },
                SnapApp { name: "vlc-media", version: "3.0.18", size_mb: 64, publisher: "VideoLAN*", description: "Universal multimedia & stream player", cmd_hint: "vlc demo.mp4", installed: false },
            ],
            profiles: vec![
                RoleProfile {
                    id: "ai-dev",
                    name: "AI Developer",
                    role_badge: "AI/ML & PyTorch",
                    venv_name: "ai-dev",
                    description: "Ephemeral .venv with PyTorch, NumPy, Transformers, ONNX & VS Code Terminal IDE.",
                    apt_deps: &["python3-ml", "tensor-bench", "curl", "vim-tiny"],
                    snap_deps: &["pytorch-lite", "vscode-oss"],
                    pip_modules: &[
                        "torch==2.1.0",
                        "numpy==1.26.4",
                        "transformers==4.40.0",
                        "scikit-learn==1.4.2",
                        "onnxruntime==1.17.1",
                        "jupyter-console==6.6.3",
                    ],
                    sample_cmd: "code train_ai.py",
                },
                RoleProfile {
                    id: "sys-kernel",
                    name: "Systems & Kernel Dev",
                    role_badge: "Rust UEFI / OS",
                    venv_name: "sys-kernel",
                    description: "Ephemeral .venv with process profiler, net-tools, VS Code & Docker build sandbox.",
                    apt_deps: &["htop", "neofetch", "net-tools", "vim-tiny"],
                    snap_deps: &["vscode-oss", "docker-cli"],
                    pip_modules: &[
                        "rustc-uefi==1.82.0",
                        "cargo-binutils==0.3.6",
                        "gdb-multiarch==13.2",
                        "qemu-trace==8.2.0",
                    ],
                    sample_cmd: "htop",
                },
                RoleProfile {
                    id: "sec-netops",
                    name: "Cybersecurity & NetOps",
                    role_badge: "Network & Audit",
                    venv_name: "sec-netops",
                    description: "Ephemeral .venv with packet telemetry, netstat/route, curl, Docker & Firefox.",
                    apt_deps: &["net-tools", "curl", "htop"],
                    snap_deps: &["firefox-esr", "docker-cli"],
                    pip_modules: &[
                        "scapy==2.5.0",
                        "nmap-lite==7.94",
                        "tshark-cli==4.2.0",
                        "cryptography==42.0.5",
                    ],
                    sample_cmd: "netstat",
                },
                RoleProfile {
                    id: "data-mlops",
                    name: "Data Science & MLOps",
                    role_badge: "Pipelines & API",
                    venv_name: "data-mlops",
                    description: "Ephemeral .venv with Python ML stack, Pandas, FastAPI, Docker & PyTorch serving.",
                    apt_deps: &["python3-ml", "curl", "htop"],
                    snap_deps: &["vscode-oss", "docker-cli", "pytorch-lite"],
                    pip_modules: &[
                        "pandas==2.2.1",
                        "scipy==1.13.0",
                        "fastapi==0.110.0",
                        "mlflow-lite==2.12.1",
                    ],
                    sample_cmd: "python3",
                },
            ],
            active_profile: None,
            profile_temp_apt: Vec::new(),
            profile_temp_snap: Vec::new(),
            extra_pip_modules: Vec::new(),
            wifi_networks: nets,
            lan_connected: true,
            lan_ip: "10.0.2.15/24",
            wifi_connected,
            wifi_ip,
            gateway_ip,
            base_ping_tenths_ms,
            active_ssid: active,
            custom_ssid: None,
            authenticated_ssids: Vec::new(),
            last_ping_ms: (base_ping_tenths_ms / 10).max(1),
        }
    }

    pub fn active_profile_info(&self) -> Option<&RoleProfile> {
        self.active_profile.and_then(|idx| self.profiles.get(idx))
    }

    pub fn active_venv_label(&self) -> Option<&'static str> {
        self.active_profile_info().map(|p| p.venv_name)
    }

    /// Activates a role profile: tears down any previous ephemeral profile first,
    /// then provisions temporary APT/Snap packages inside the profile's `.venv`.
    pub fn activate_profile(&mut self, idx: usize) -> Option<(&'static str, usize, usize)> {
        if idx >= self.profiles.len() {
            return None;
        }
        self.deactivate_profile();

        let apt_deps = self.profiles[idx].apt_deps;
        let snap_deps = self.profiles[idx].snap_deps;
        let name = self.profiles[idx].name;

        let mut added_apt = 0usize;
        for &pkg_name in apt_deps {
            if let Some(pkg) = self.apt_packages.iter_mut().find(|p| p.name == pkg_name) {
                if !pkg.installed {
                    pkg.installed = true;
                    self.profile_temp_apt.push(pkg.name);
                    added_apt += 1;
                }
            }
        }

        let mut added_snap = 0usize;
        for &app_name in snap_deps {
            if let Some(app) = self.snap_apps.iter_mut().find(|a| a.name == app_name) {
                if !app.installed {
                    app.installed = true;
                    self.profile_temp_snap.push(app.name);
                    added_snap += 1;
                }
            }
        }

        self.active_profile = Some(idx);
        Some((name, added_apt, added_snap))
    }

    /// Deactivates the active profile and deletes all temporarily installed `.venv`
    /// packages & Python modules from the system.
    pub fn deactivate_profile(&mut self) -> Option<(&'static str, usize)> {
        let idx = self.active_profile.take()?;
        let name = self.profiles.get(idx).map(|p| p.name).unwrap_or("Profile");
        let mut removed = 0usize;

        for pkg_name in self.profile_temp_apt.drain(..) {
            if let Some(pkg) = self.apt_packages.iter_mut().find(|p| p.name == pkg_name) {
                if pkg.installed {
                    pkg.installed = false;
                    removed += 1;
                }
            }
        }

        for app_name in self.profile_temp_snap.drain(..) {
            if let Some(app) = self.snap_apps.iter_mut().find(|a| a.name == app_name) {
                if app.installed {
                    app.installed = false;
                    removed += 1;
                }
            }
        }

        removed += self.extra_pip_modules.len();
        self.extra_pip_modules.clear();

        Some((name, removed))
    }

    pub fn get_active_ssid(&self) -> &str {
        if !self.wifi_connected {
            return "Disconnected";
        }
        if let Some(custom) = &self.custom_ssid {
            custom.as_str()
        } else {
            &self.active_ssid
        }
    }

    pub fn connect_custom_wifi(&mut self, ssid: &str) {
        self.active_ssid = String::from(ssid);
        self.custom_ssid = Some(String::from(ssid));
        self.wifi_connected = true;
        for net in self.wifi_networks.iter_mut() {
            net.connected = net.ssid == ssid;
        }
    }

    pub fn validate_wifi_password(
        &mut self,
        ssid: &str,
        security: &str,
        password: &str,
    ) -> Result<(), &'static str> {
        let sec = security.trim();
        if sec.is_empty() || sec.eq_ignore_ascii_case("open") || sec.eq_ignore_ascii_case("none") {
            return Ok(());
        }
        if password.is_empty() {
            return Err("Authentication failed: passphrase required for locked network.");
        }
        if (sec.contains("WPA") || sec.contains("SAE") || sec.contains("EAP")) && password.len() < 8 {
            return Err("Authentication failed: WPA passphrase must be at least 8 characters.");
        }
        if sec.eq_ignore_ascii_case("wep") && password.len() < 5 {
            return Err("Authentication failed: WEP key must be at least 5 characters.");
        }
        if let Some((_, saved)) = self.authenticated_ssids.iter().find(|(s, _)| s == ssid) {
            if saved != password {
                return Err("Authentication failed: incorrect password for network.");
            }
            return Ok(());
        }
        self.authenticated_ssids
            .push((String::from(ssid), String::from(password)));
        Ok(())
    }

    pub fn dpkg_status(&self) -> String {
        let mut out = String::new();
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
                return Some((pkg.name, true));
            }
            pkg.installed = !pkg.installed;
            if !pkg.installed {
                self.profile_temp_apt.retain(|&n| n != pkg.name);
            }
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
                self.profile_temp_apt.retain(|&n| n != pkg.name);
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
            if !app.installed {
                self.profile_temp_snap.retain(|&n| n != app.name);
            }
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

    pub fn remove_snap_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        if name == "snap-store" {
            return Err("error: cannot remove essential snap 'snap-store'.");
        }
        for app in &mut self.snap_apps {
            if app.name == name {
                if !app.installed {
                    return Err("snap is not installed.");
                }
                app.installed = false;
                self.profile_temp_snap.retain(|&n| n != app.name);
                return Ok(());
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
        let loaded = load_device_wifi();
        let mut fresh = loaded.networks;
        if let Some(ip) = loaded.ip {
            self.wifi_ip = ip;
        }
        if let Some(gw) = loaded.gateway {
            self.gateway_ip = gw;
        }
        if let Some(bp) = loaded.base_ping_tenths_ms {
            self.base_ping_tenths_ms = bp;
        }
        let tsc = crate::timer::read_tsc();
        for (i, net) in fresh.iter_mut().enumerate() {
            let delta = ((tsc.wrapping_add(i as u64 * 17) % 5) as i16) - 2;
            let new_sig = (net.signal_pct as i16 + delta).clamp(15, 99) as u8;
            net.signal_pct = new_sig;
            if self.custom_ssid.is_none() && self.wifi_connected {
                net.connected = net.ssid == self.active_ssid;
            }
        }
        self.wifi_networks = fresh;
        if self.wifi_connected && self.custom_ssid.is_none() {
            if let Some(conn) = self.wifi_networks.iter().find(|n| n.connected) {
                self.active_ssid = conn.ssid.clone();
            }
        }
    }

    pub fn toggle_lan(&mut self) {
        self.lan_connected = !self.lan_connected;
    }

    /// Computes realistic per-network and per-website ping telemetry:
    /// Returns `(resolved_host, ip_str, ttl, base_rtt_ms, jitter_scale_ms)`.
    pub fn compute_ping_metrics(&self, raw_target: &str) -> (String, String, u8, f32, f32) {
        let stripped = raw_target
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let clean = stripped.split('/').next().unwrap_or(stripped).trim();
        let lower = clean.to_ascii_lowercase();

        // FNV-1a 64-bit hash for deterministic yet distinct per-domain routing metrics
        let mut h: u64 = 0xcbf29ce484222325;
        for b in lower.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        // Link-layer latency based on currently active Wi-Fi signal/band vs Gigabit LAN
        let link_overhead_ms = if self.wifi_connected {
            let (sig, is_24g) = self
                .wifi_networks
                .iter()
                .find(|n| n.connected || n.ssid == self.get_active_ssid())
                .map(|n| (n.signal_pct as f32, n.band.starts_with("2.4")))
                .unwrap_or((72.0, false));
            let signal_penalty = (100.0 - sig.clamp(15.0, 99.0)) * 0.28;
            let band_penalty = if is_24g { 3.8 } else { 1.1 };
            let custom_offset = if self.custom_ssid.is_some() { 4.2 } else { 0.0 };
            signal_penalty + band_penalty + custom_offset
        } else {
            0.55 // Wired Gigabit Ethernet eth0
        };

        let host_wan_base = if self.base_ping_tenths_ms > 0 {
            (self.base_ping_tenths_ms as f32 / 10.0 - 8.5).max(4.0)
        } else {
            11.0
        };
        let wan_base = host_wan_base + link_overhead_ms;

        if lower == "localhost" || lower == "127.0.0.1" {
            return (String::from("localhost"), String::from("127.0.0.1"), 64, 0.04, 0.02);
        }

        let gw = if self.gateway_ip.is_empty() || self.gateway_ip == "0.0.0.0" {
            "192.168.1.1"
        } else {
            self.gateway_ip.as_str()
        };
        if lower == "gateway" || lower == "router" || lower == "192.168.1.1" || lower == gw {
            return (
                String::from("gateway.local"),
                String::from(gw),
                64,
                (0.9 + link_overhead_ms * 0.45).max(0.4),
                (0.35 + link_overhead_ms * 0.08).min(4.0),
            );
        }

        match lower.as_str() {
            "1.1.1.1" | "cloudflare.com" | "www.cloudflare.com" => {
                (String::from("one.one.one.one"), String::from("1.1.1.1"), 58, (wan_base - 4.2).max(3.5), 0.9)
            }
            "8.8.8.8" | "dns.google" => {
                (String::from("dns.google"), String::from("8.8.8.8"), 117, wan_base, 1.1)
            }
            "google.com" | "www.google.com" => {
                (String::from("google.com"), String::from("142.250.190.46"), 116, wan_base + 3.4, 1.4)
            }
            "youtube.com" | "www.youtube.com" => {
                (String::from("youtube.com"), String::from("142.250.193.206"), 115, wan_base + 5.8, 1.7)
            }
            "github.com" | "www.github.com" => {
                (String::from("github.com"), String::from("140.82.121.4"), 54, wan_base + 19.4, 2.3)
            }
            "openai.com" | "www.openai.com" | "api.openai.com" => {
                (String::from(clean), String::from("104.18.33.45"), 57, wan_base + 12.6, 1.8)
            }
            "huggingface.co" => {
                (String::from("huggingface.co"), String::from("18.172.134.81"), 53, wan_base + 24.7, 2.5)
            }
            "pytorch.org" | "www.pytorch.org" => {
                (String::from("pytorch.org"), String::from("185.199.108.153"), 55, wan_base + 21.3, 2.1)
            }
            "deb.debian.org" | "debian.org" => {
                (String::from("deb.debian.org"), String::from("151.101.2.132"), 56, wan_base + 9.2, 1.6)
            }
            "zenkai-os.org" | "deb.zenkai-os.org" => {
                (String::from(clean), String::from("104.21.48.209"), 59, wan_base + 6.7, 1.3)
            }
            "jainuniversity.ac.in" | "www.jainuniversity.ac.in" => {
                (String::from(clean), String::from("103.21.58.114"), 60, wan_base + 7.9, 1.5)
            }
            _ => {
                let is_ipv4 = lower.split('.').count() == 4
                    && lower.split('.').all(|oct| oct.parse::<u8>().is_ok());
                if is_ipv4 {
                    let is_private = lower.starts_with("192.168.")
                        || lower.starts_with("10.")
                        || lower.starts_with("172.");
                    if is_private {
                        (
                            String::from(clean),
                            String::from(clean),
                            64,
                            (1.2 + link_overhead_ms * 0.5).max(0.5),
                            0.6,
                        )
                    } else {
                        let extra = ((h % 45) as f32) * 0.85;
                        let ttl = if (h & 1) == 0 { 54 } else { 116 };
                        (String::from(clean), String::from(clean), ttl, wan_base + extra, 1.8)
                    }
                } else {
                    let a_choices = [13u8, 18, 34, 52, 54, 104, 142, 151, 172, 185, 198, 203];
                    let a = a_choices[(h as usize) % a_choices.len()];
                    let b = (((h >> 8) & 0xFE) as u8).max(1);
                    let c = (((h >> 16) & 0xFE) as u8).max(1);
                    let d = (((h >> 24) & 0xFD) as u8).max(2);
                    let ip_str = format!("{}.{}.{}.{}", a, b, c, d);
                    let ttls = [49u8, 52, 54, 57, 60, 114, 118, 121];
                    let ttl = ttls[((h >> 32) as usize) % ttls.len()];
                    let extra = 4.0 + ((h % 68) as f32) * 0.78;
                    let j_scale = 1.2 + (((h >> 12) % 22) as f32) * 0.1;
                    (String::from(clean), ip_str, ttl, wan_base + extra, j_scale)
                }
            }
        }
    }
}
