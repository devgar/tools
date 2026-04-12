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
    read_battery_state_from(battery_name, "/sys/class/power_supply").await
}

async fn read_battery_state_from(battery_name: &str, base: &str) -> anyhow::Result<BatteryState> {
    use tokio::fs;

    let capacity = fs::read_to_string(format!("{base}/{battery_name}/capacity"))
        .await?
        .trim()
        .parse::<u8>()
        .unwrap_or(0);

    let status = fs::read_to_string(format!("{base}/{battery_name}/status"))
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── battery_icon ──────────────────────────────────────────────────────────

    #[test]
    fn icon_charging_overrides_level_at_zero() {
        assert_eq!(battery_icon(0, "Charging"), "");
    }

    #[test]
    fn icon_charging_overrides_level_at_50() {
        assert_eq!(battery_icon(50, "Charging"), "");
    }

    #[test]
    fn icon_charging_overrides_level_at_100() {
        assert_eq!(battery_icon(100, "Charging"), "");
    }

    #[test]
    fn icon_full_above_90() {
        assert_eq!(battery_icon(91, "Discharging"), "");
        assert_eq!(battery_icon(100, "Discharging"), "");
    }

    #[test]
    fn icon_boundary_at_90_is_not_full() {
        // capacity > 90 is false at exactly 90, so it falls to the next tier
        assert_eq!(battery_icon(90, "Discharging"), "");
    }

    #[test]
    fn icon_high_above_70() {
        assert_eq!(battery_icon(71, "Discharging"), "");
        assert_eq!(battery_icon(85, "Discharging"), "");
    }

    #[test]
    fn icon_boundary_at_70_is_not_high() {
        assert_eq!(battery_icon(70, "Discharging"), "");
    }

    #[test]
    fn icon_medium_above_50() {
        assert_eq!(battery_icon(51, "Discharging"), "");
        assert_eq!(battery_icon(65, "Discharging"), "");
    }

    #[test]
    fn icon_boundary_at_50_is_not_medium() {
        assert_eq!(battery_icon(50, "Discharging"), "");
    }

    #[test]
    fn icon_low_above_30() {
        assert_eq!(battery_icon(31, "Discharging"), "");
        assert_eq!(battery_icon(45, "Discharging"), "");
    }

    #[test]
    fn icon_boundary_at_30_is_not_low() {
        assert_eq!(battery_icon(30, "Discharging"), "");
    }

    #[test]
    fn icon_critical_above_10() {
        assert_eq!(battery_icon(11, "Discharging"), "");
        assert_eq!(battery_icon(20, "Discharging"), "");
    }

    #[test]
    fn icon_boundary_at_10_is_empty() {
        assert_eq!(battery_icon(10, "Discharging"), "");
    }

    #[test]
    fn icon_empty_at_zero() {
        assert_eq!(battery_icon(0, "Discharging"), "");
    }

    #[test]
    fn icon_unknown_status_uses_level() {
        // Unknown status (not "Charging") falls through to capacity checks
        assert_eq!(battery_icon(95, "Unknown"), "");
        assert_eq!(battery_icon(5, "Full"), "");
    }

    // ── read_battery_state_from ───────────────────────────────────────────────

    fn make_sysfs(dir: &std::path::Path, name: &str, capacity: &str, status: &str) {
        let bat = dir.join(name);
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("capacity"), capacity).unwrap();
        std::fs::write(bat.join("status"), status).unwrap();
    }

    #[tokio::test]
    async fn sysfs_normal_values() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "BAT0", "85\n", "Discharging\n");
        let state = read_battery_state_from("BAT0", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 85);
        assert_eq!(state.status, "Discharging");
        assert_eq!(state.icon, ""); // > 70
    }

    #[tokio::test]
    async fn sysfs_no_trailing_newline() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "BAT0", "42", "Charging");
        let state = read_battery_state_from("BAT0", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 42);
        assert_eq!(state.status, "Charging");
        assert_eq!(state.icon, "");
    }

    #[tokio::test]
    async fn sysfs_non_numeric_capacity_defaults_to_zero() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "BAT0", "N/A\n", "Unknown\n");
        let state = read_battery_state_from("BAT0", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 0);
        assert_eq!(state.icon, ""); // capacity 0, not Charging
    }

    #[tokio::test]
    async fn sysfs_missing_files_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        // No files created — directory doesn't even exist
        let result = read_battery_state_from("BAT0", tmp.path().to_str().unwrap()).await;
        assert!(result.is_err());
    }

    // ── watch smoke test ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn watch_does_not_hang_on_dropped_receiver() {
        let provider = SysfsBatteryProvider { battery_name: "BAT0".to_string() };
        drop(provider.watch());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
