use crate::domain::{BrightnessState, StateProvider};
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct SysfsBacklightProvider {
    device: String,
}

#[async_trait]
impl StateProvider<BrightnessState> for SysfsBacklightProvider {
    fn path(&self) -> &'static str {
        "system.brightness"
    }

    async fn init(&self) -> anyhow::Result<BrightnessState> {
        read_brightness(&self.device, "/sys/class/backlight").await
    }

    fn watch(&self) -> mpsc::Receiver<BrightnessState> {
        let device = self.device.clone();
        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::process::Command;

            let mut child = match Command::new("udevadm")
                .args(["monitor", "--udev", "--subsystem-match=backlight"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ewwkit: failed to spawn `udevadm monitor`: {e}");
                    return;
                }
            };

            let stdout = child.stdout.take().unwrap();
            let mut lines = BufReader::new(stdout).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // "UDEV  [timestamp] change   /devices/.../backlight/... (backlight)"
                if line.contains("change") {
                    if let Ok(state) = read_brightness(&device, "/sys/class/backlight").await {
                        if tx.send(state).await.is_err() {
                            break;
                        }
                    }
                }
            }

            let _ = child.kill().await;
        });

        rx
    }
}

async fn read_brightness(device: &str, base: &str) -> anyhow::Result<BrightnessState> {
    read_brightness_from(device, base).await
}

async fn read_brightness_from(device: &str, base: &str) -> anyhow::Result<BrightnessState> {
    use tokio::fs;

    let raw = fs::read_to_string(format!("{base}/{device}/brightness"))
        .await?
        .trim()
        .parse::<u32>()
        .unwrap_or(0);

    let max = fs::read_to_string(format!("{base}/{device}/max_brightness"))
        .await?
        .trim()
        .parse::<u32>()
        .unwrap_or(1);

    let level = if max > 0 {
        ((raw * 100) / max).min(100) as u8
    } else {
        0
    };

    Ok(BrightnessState {
        level,
        icon: brightness_icon(level),
    })
}

fn brightness_icon(level: u8) -> String {
    if level >= 75 { "󰃠" }
    else if level >= 50 { "󰃟" }
    else if level >= 25 { "󰃞" }
    else { "󰃝" }
    .into()
}

fn discover_backlight() -> Option<String> {
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

pub fn create_brightness_provider() -> Option<impl StateProvider<BrightnessState>> {
    discover_backlight().map(|device| SysfsBacklightProvider { device })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── brightness_icon ───────────────────────────────────────────────────────

    #[test]
    fn icon_full_at_75() {
        assert_eq!(brightness_icon(75), "󰃠");
        assert_eq!(brightness_icon(100), "󰃠");
    }

    #[test]
    fn icon_boundary_just_below_75() {
        assert_eq!(brightness_icon(74), "󰃟");
    }

    #[test]
    fn icon_medium_at_50() {
        assert_eq!(brightness_icon(50), "󰃟");
        assert_eq!(brightness_icon(60), "󰃟");
    }

    #[test]
    fn icon_boundary_just_below_50() {
        assert_eq!(brightness_icon(49), "󰃞");
    }

    #[test]
    fn icon_low_at_25() {
        assert_eq!(brightness_icon(25), "󰃞");
        assert_eq!(brightness_icon(30), "󰃞");
    }

    #[test]
    fn icon_boundary_just_below_25() {
        assert_eq!(brightness_icon(24), "󰃝");
    }

    #[test]
    fn icon_dim_at_zero() {
        assert_eq!(brightness_icon(0), "󰃝");
    }

    // ── read_brightness_from ──────────────────────────────────────────────────

    fn make_sysfs(dir: &std::path::Path, device: &str, brightness: &str, max: &str) {
        let dev = dir.join(device);
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("brightness"), brightness).unwrap();
        std::fs::write(dev.join("max_brightness"), max).unwrap();
    }

    #[tokio::test]
    async fn sysfs_calculates_percentage() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "intel_backlight", "750\n", "1000\n");
        let state = read_brightness_from("intel_backlight", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 75);
        assert_eq!(state.icon, "󰃠");
    }

    #[tokio::test]
    async fn sysfs_full_brightness() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "intel_backlight", "1000\n", "1000\n");
        let state = read_brightness_from("intel_backlight", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 100);
    }

    #[tokio::test]
    async fn sysfs_no_trailing_newline() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "bl", "500", "1000");
        let state = read_brightness_from("bl", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 50);
    }

    #[tokio::test]
    async fn sysfs_non_numeric_defaults_to_zero() {
        let tmp = tempfile::tempdir().unwrap();
        make_sysfs(tmp.path(), "bl", "N/A\n", "1000\n");
        let state = read_brightness_from("bl", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 0);
    }

    #[tokio::test]
    async fn sysfs_missing_files_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_brightness_from("nonexistent", tmp.path().to_str().unwrap()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sysfs_caps_at_100() {
        let tmp = tempfile::tempdir().unwrap();
        // raw > max would be unusual but should not panic or overflow
        make_sysfs(tmp.path(), "bl", "1200\n", "1000\n");
        let state = read_brightness_from("bl", tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(state.level, 100);
    }

    // ── watch smoke test ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn watch_does_not_hang_on_dropped_receiver() {
        let provider = SysfsBacklightProvider {
            device: "intel_backlight".to_string(),
        };
        drop(provider.watch());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
