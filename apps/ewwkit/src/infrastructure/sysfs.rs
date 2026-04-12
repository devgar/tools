use crate::domain::{SystemProvider, BatteryState};
use async_trait::async_trait;
use std::fs;

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



}
