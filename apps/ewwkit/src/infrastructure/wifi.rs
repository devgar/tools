use crate::domain::{NetworkState, WifiProvider};
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct NmcliWifiProvider;

#[async_trait]
impl WifiProvider for NmcliWifiProvider {
    async fn get_network(&self) -> anyhow::Result<NetworkState> {
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

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        // Format: "yes:SSID:75" — use rsplit_once to handle SSIDs that contain colons.
        if let Some(rest) = line.strip_prefix("yes:") {
            if let Some((ssid, sig)) = rest.rsplit_once(':') {
                let signal = sig.parse::<u8>().unwrap_or(0);
                return Ok(NetworkState {
                    wifi_ssid: ssid.to_string(),
                    signal,
                    icon: wifi_icon(signal),
                });
            }
        }
    }

    Ok(NetworkState {
        wifi_ssid: "Disconnected".into(),
        signal: 0,
        icon: "󰤭".into(),
    })
}

fn wifi_icon(signal: u8) -> String {
    if signal >= 75 { "󰤨" }
    else if signal >= 50 { "󰤥" }
    else if signal >= 25 { "󰤢" }
    else { "󰤟" }
    .into()
}

pub fn create_wifi_provider() -> impl WifiProvider {
    NmcliWifiProvider
}
