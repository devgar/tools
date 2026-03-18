use crate::domain::{SystemProvider, BatteryState, NetworkState, AudioState};
use async_trait::async_trait;
use std::fs;
use tokio::process::Command;

pub struct SysfsAdapter {
    pub battery_name: String,
}

impl SysfsAdapter {
    pub fn new(battery_name: &str) -> Self {
        Self {
            battery_name: battery_name.to_string(),
        }
    }

    fn read_sysfs_battery(&self, file: &str) -> anyhow::Result<String> {
        let path = format!("/sys/class/power_supply/{}/{}", self.battery_name, file);
        if !fs::metadata(&path).is_ok() {
            return Err(anyhow::anyhow!("Battery file not found: {}", path));
        }
        Ok(fs::read_to_string(path)?.trim().to_string())
    }
}

#[async_trait]
impl SystemProvider for SysfsAdapter {
    async fn get_battery(&self) -> anyhow::Result<BatteryState> {
        let capacity = self.read_sysfs_battery("capacity")?.parse::<u8>().unwrap_or(0);
        let status = self.read_sysfs_battery("status")?;
        
        let icon = if status == "Charging" {
            ""
        } else if capacity > 90 {
            ""
        } else if capacity > 70 {
            ""
        } else if capacity > 50 {
            ""
        } else if capacity > 30 {
            ""
        } else if capacity > 10 {
            ""
        } else {
            ""
        };

        Ok(BatteryState {
            level: capacity,
            status,
            icon: icon.to_string(),
        })
    }

    async fn get_network(&self) -> anyhow::Result<NetworkState> {
        // Leemos ssid de forma más eficiente si es posible, o mantenemos nmcli pero con menos frecuencia
        // Por ahora, para reducir CPU, intentaremos leer info básica de sysfs
        let operstate = fs::read_to_string("/sys/class/net/wlp3s0/operstate").unwrap_or_else(|_| "down".into());
        
        if operstate.trim() == "up" {
             // Solo si está up, podemos intentar sacar el SSID (esto sigue siendo caro con nmcli,
             // pero al menos evitamos ejecutarlo si la interfaz está caída)
             // Optimización real: Usar una caché o bajar frecuencia en main.rs
             return Ok(NetworkState {
                wifi_ssid: "Connected".into(), // Placeholder hasta optimizar nmcli
                signal: 75,
                icon: "󰤨".into(),
            });
        }

        Ok(NetworkState {
            wifi_ssid: "Disconnected".into(),
            signal: 0,
            icon: "󰤭".into(),
        })
    }

    async fn get_audio(&self) -> anyhow::Result<AudioState> {
        let output = Command::new("amixer")
            .args(["sget", "Master"])
            .output()
            .await?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().last() {
                let vol = line.split('[')
                    .nth(1)
                    .and_then(|s| s.split('%').next())
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(0);
                
                let muted = line.contains("[off]");
                
                return Ok(AudioState {
                    volume: vol,
                    muted,
                });
            }
        }

        Ok(AudioState { volume: 0, muted: true })
    }
}
