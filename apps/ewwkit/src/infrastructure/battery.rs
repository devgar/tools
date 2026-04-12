use crate::domain::{BatteryProvider, BatteryState};
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct SysfsBatteryProvider {
    battery_name: String,
}

#[async_trait]
impl BatteryProvider for SysfsBatteryProvider {
    async fn get_battery(&self) -> anyhow::Result<BatteryState> {
        read_battery_state(&self.battery_name).await
    }

    fn watch(&self) -> mpsc::Receiver<BatteryState> {
        let battery_name = self.battery_name.clone();
        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::process::Command;

            let mut child = match Command::new("upower")
                .args(["--monitor"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ewwkit: failed to spawn `upower --monitor`: {e}");
                    return;
                }
            };

            let stdout = child.stdout.take().unwrap();
            let mut lines = BufReader::new(stdout).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // Event lines start with a timestamp bracket: "[HH:MM:SS.mmm] Device changed: ..."
                // Any device event (battery, AC adapter, display device) can affect battery state.
                if line.starts_with('[') {
                    match read_battery_state(&battery_name).await {
                        Ok(bat) => {
                            if tx.send(bat).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => eprintln!("ewwkit: battery read error: {e}"),
                    }
                }
            }

            let _ = child.kill().await;
        });

        rx
    }
}

async fn read_battery_state(battery_name: &str) -> anyhow::Result<BatteryState> {
    use tokio::fs;

    let capacity = fs::read_to_string(format!(
        "/sys/class/power_supply/{battery_name}/capacity"
    ))
    .await?
    .trim()
    .parse::<u8>()
    .unwrap_or(0);

    let status = fs::read_to_string(format!(
        "/sys/class/power_supply/{battery_name}/status"
    ))
    .await?
    .trim()
    .to_string();

    let icon = battery_icon(capacity, &status);
    Ok(BatteryState { level: capacity, status, icon })
}

fn battery_icon(capacity: u8, status: &str) -> String {
    if status == "Charging" { "" }
    else if capacity > 90 { "" }
    else if capacity > 70 { "" }
    else if capacity > 50 { "" }
    else if capacity > 30 { "" }
    else if capacity > 10 { "" }
    else { "" }
    .into()
}

fn discover_battery() -> Option<String> {
    std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .find(|entry| {
            std::fs::read_to_string(entry.path().join("type"))
                .map(|t| t.trim() == "Battery")
                .unwrap_or(false)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

pub fn create_battery_provider() -> impl BatteryProvider {
    let battery_name = discover_battery().unwrap_or_else(|| {
        eprintln!("ewwkit: no battery found in /sys/class/power_supply, defaulting to BAT0");
        "BAT0".to_string()
    });
    SysfsBatteryProvider { battery_name }
}
