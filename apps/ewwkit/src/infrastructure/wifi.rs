use crate::domain::{NetworkState, StateProvider};
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct NmcliWifiProvider;

#[async_trait]
impl StateProvider<NetworkState> for NmcliWifiProvider {
    fn path(&self) -> &'static str {
        "system.network"
    }

    async fn init(&self) -> anyhow::Result<NetworkState> {
        query_nmcli().await
    }

    fn watch(&self) -> mpsc::Receiver<NetworkState> {
        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::process::Command;

            let mut child = match Command::new("nmcli")
                .args(["monitor"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ewwkit: failed to spawn `nmcli monitor`: {e}");
                    return;
                }
            };

            let stdout = child.stdout.take().unwrap();
            let mut lines = BufReader::new(stdout).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // "wlp3s0: connected"
                // "wlp3s0: disconnected"
                // "wlp3s0: connecting (getting IP configuration)"
                // "Connectivity is now 'full'"
                // "Connectivity is now 'none'"
                let relevant = line.contains(": connected")
                    || line.contains(": disconnected")
                    || line.contains(": connecting")
                    || line.starts_with("Connectivity is now");

                if relevant {
                    if let Ok(net) = query_nmcli().await {
                        if tx.send(net).await.is_err() {
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

async fn query_nmcli() -> anyhow::Result<NetworkState> {
    use tokio::process::Command;

    let output = Command::new("nmcli")
        .args(["-t", "-f", "active,ssid,signal", "dev", "wifi"])
        .output()
        .await?;

    Ok(parse_nmcli_output(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_nmcli_output(stdout: &str) -> NetworkState {
    for line in stdout.lines() {
        // Format: "yes:SSID:75" — use rsplit_once to handle SSIDs that contain colons.
        if let Some(rest) = line.strip_prefix("yes:") {
            if let Some((ssid, sig)) = rest.rsplit_once(':') {
                let signal = sig.parse::<u8>().unwrap_or(0);
                return NetworkState {
                    wifi_ssid: ssid.to_string(),
                    signal,
                    icon: wifi_icon(signal),
                };
            }
        }
    }

    NetworkState {
        wifi_ssid: "Disconnected".into(),
        signal: 0,
        icon: "󰤭".into(),
    }
}

fn wifi_icon(signal: u8) -> String {
    if signal >= 75 { "󰤨" }
    else if signal >= 50 { "󰤥" }
    else if signal >= 25 { "󰤢" }
    else { "󰤟" }
    .into()
}

pub fn create_wifi_provider() -> impl StateProvider<NetworkState> {
    NmcliWifiProvider
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::StateProvider;

    // ── wifi_icon ─────────────────────────────────────────────────────────────

    #[test]
    fn icon_strong_at_75() {
        assert_eq!(wifi_icon(75), "󰤨");
        assert_eq!(wifi_icon(100), "󰤨");
    }

    #[test]
    fn icon_boundary_just_below_75() {
        assert_eq!(wifi_icon(74), "󰤥");
    }

    #[test]
    fn icon_good_at_50() {
        assert_eq!(wifi_icon(50), "󰤥");
        assert_eq!(wifi_icon(60), "󰤥");
    }

    #[test]
    fn icon_boundary_just_below_50() {
        assert_eq!(wifi_icon(49), "󰤢");
    }

    #[test]
    fn icon_fair_at_25() {
        assert_eq!(wifi_icon(25), "󰤢");
        assert_eq!(wifi_icon(30), "󰤢");
    }

    #[test]
    fn icon_boundary_just_below_25() {
        assert_eq!(wifi_icon(24), "󰤟");
    }

    #[test]
    fn icon_weak_at_zero() {
        assert_eq!(wifi_icon(0), "󰤟");
    }

    // ── parse_nmcli_output ────────────────────────────────────────────────────

    #[test]
    fn parse_simple_connected_ssid() {
        let out = "no:OtherNetwork:80\nyes:HomeWifi:65\n";
        let state = parse_nmcli_output(out);
        assert_eq!(state.wifi_ssid, "HomeWifi");
        assert_eq!(state.signal, 65);
        assert_eq!(state.icon, "󰤥"); // 50 ≤ 65 < 75
    }

    #[test]
    fn parse_ssid_with_colons() {
        // rsplit_once takes the LAST colon, so everything before it is the SSID
        let out = "yes:My:Colon:SSID:82\n";
        let state = parse_nmcli_output(out);
        assert_eq!(state.wifi_ssid, "My:Colon:SSID");
        assert_eq!(state.signal, 82);
        assert_eq!(state.icon, "󰤨");
    }

    #[test]
    fn parse_no_active_connection_returns_disconnected() {
        let out = "no:SomeNetwork:50\nno:OtherNetwork:70\n";
        let state = parse_nmcli_output(out);
        assert_eq!(state.wifi_ssid, "Disconnected");
        assert_eq!(state.signal, 0);
        assert_eq!(state.icon, "󰤭");
    }

    #[test]
    fn parse_empty_output_returns_disconnected() {
        let state = parse_nmcli_output("");
        assert_eq!(state.wifi_ssid, "Disconnected");
        assert_eq!(state.signal, 0);
        assert_eq!(state.icon, "󰤭");
    }

    #[test]
    fn parse_malformed_signal_defaults_to_zero() {
        let out = "yes:MySSID:notanumber\n";
        let state = parse_nmcli_output(out);
        assert_eq!(state.wifi_ssid, "MySSID");
        assert_eq!(state.signal, 0);
        assert_eq!(state.icon, "󰤟");
    }

    #[test]
    fn parse_signal_icon_boundaries() {
        assert_eq!(parse_nmcli_output("yes:S:75\n").icon, "󰤨");
        assert_eq!(parse_nmcli_output("yes:S:50\n").icon, "󰤥");
        assert_eq!(parse_nmcli_output("yes:S:25\n").icon, "󰤢");
        assert_eq!(parse_nmcli_output("yes:S:24\n").icon, "󰤟");
    }

    #[test]
    fn parse_takes_first_yes_line() {
        let out = "yes:FirstSSID:80\nyes:SecondSSID:60\n";
        let state = parse_nmcli_output(out);
        assert_eq!(state.wifi_ssid, "FirstSSID");
    }

    // ── watch smoke test ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn watch_does_not_hang_on_dropped_receiver() {
        let provider = NmcliWifiProvider;
        drop(provider.watch());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
