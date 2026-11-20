use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use sysinfo::{Networks, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareStats {
    pub cpu_percent: f32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_percent: f32,
    pub net_down_mbps: f64,
    pub net_up_mbps: f64,
    pub is_fullscreen: bool,
}

pub struct HardwareMonitor {
    sys: Mutex<System>,
    networks: Mutex<Networks>,
}

impl HardwareMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let networks = Networks::new_with_refreshed_list();
        Self {
            sys: Mutex::new(sys),
            networks: Mutex::new(networks),
        }
    }

    pub fn sample(&self) -> HardwareStats {
        let (cpu_percent, ram_used_gb, ram_total_gb, ram_percent) = {
            let mut sys = self.sys.lock().unwrap();
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let cpu = sys.global_cpu_usage();
            let used = sys.used_memory() as f64 / 1_073_741_824.0;
            let total = (sys.total_memory() as f64 / 1_073_741_824.0).max(0.1);
            let pct = ((used / total) * 100.0) as f32;
            (cpu, used, total, pct)
        };

        let (net_down_mbps, net_up_mbps) = {
            let mut nets = self.networks.lock().unwrap();
            nets.refresh();
            let mut rx: u64 = 0;
            let mut tx: u64 = 0;
            for (_name, data) in nets.iter() {
                rx += data.received();
                tx += data.transmitted();
            }
            // Convert bytes per ~1s interval into Mbps
            ((rx as f64 * 8.0) / 1_000_000.0, (tx as f64 * 8.0) / 1_000_000.0)
        };

        HardwareStats {
            cpu_percent,
            ram_used_gb: (ram_used_gb * 10.0).round() / 10.0,
            ram_total_gb: (ram_total_gb * 10.0).round() / 10.0,
            ram_percent,
            net_down_mbps: (net_down_mbps * 100.0).round() / 100.0,
            net_up_mbps: (net_up_mbps * 100.0).round() / 100.0,
            is_fullscreen: crate::win32::is_foreground_fullscreen(),
        }
    }
}
