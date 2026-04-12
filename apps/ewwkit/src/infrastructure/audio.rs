use crate::domain::{AudioProvider, AudioState};
use async_trait::async_trait;
use tokio::sync::mpsc;

// ── pactl implementation (default) ──────────────────────────────────────────
//
// Spawns `pactl subscribe` once and listens for sink change events.
// On each event, queries the current state via two fast pactl calls.
// Zero overhead at idle — no polling.

#[cfg(not(feature = "alsa"))]
pub struct PactlMonitor;

#[cfg(not(feature = "alsa"))]
#[async_trait]
impl AudioProvider for PactlMonitor {
    async fn get_audio(&self) -> anyhow::Result<AudioState> {
        query_pactl().await
    }

    fn watch(&self) -> mpsc::Receiver<AudioState> {
        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::process::Command;

            let mut child = match Command::new("pactl")
                .args(["subscribe"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ewwkit: failed to spawn `pactl subscribe`: {e}");
                    return;
                }
            };

            let stdout = child.stdout.take().unwrap();
            let mut lines = BufReader::new(stdout).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // "Event 'change' on sink #0"
                // The space before "sink #" excludes "sink-input" events.
                if line.contains("'change'") && line.contains(" on sink #") {
                    if let Ok(audio) = query_pactl().await {
                        if tx.send(audio).await.is_err() {
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

#[cfg(not(feature = "alsa"))]
async fn query_pactl() -> anyhow::Result<AudioState> {
    use tokio::process::Command;

    let vol_out = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .await?;
    let mute_out = Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .await?;

    let vol_str = String::from_utf8_lossy(&vol_out.stdout);
    let mute_str = String::from_utf8_lossy(&mute_out.stdout);

    // "Volume: front-left: 65536 /  100% / 0.00 dB,   front-right: ..."
    let volume = vol_str
        .split('/')
        .nth(1)
        .and_then(|s| s.trim().strip_suffix('%'))
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    // "Mute: yes" / "Mute: no"
    let muted = mute_str.contains("yes");

    Ok(AudioState { volume, muted })
}

// ── alsa crate implementation (--features alsa) ──────────────────────────────
//
// Opens the ALSA control interface as an AsyncFd so the event loop integrates
// directly with Tokio without blocking a thread. On each VALUE event, reads the
// mixer volume synchronously via spawn_blocking (the alsa crate is sync-only).
//
// Build requirement: alsa-lib-devel (Fedora) / libasound2-dev (Debian/Ubuntu).

#[cfg(feature = "alsa")]
pub struct AlsaMonitor;

#[cfg(feature = "alsa")]
#[async_trait]
impl AudioProvider for AlsaMonitor {
    async fn get_audio(&self) -> anyhow::Result<AudioState> {
        tokio::task::spawn_blocking(read_alsa_audio).await?
    }

    fn watch(&self) -> mpsc::Receiver<AudioState> {
        use alsa::ctl::{Ctl, EventMask};
        use tokio::io::unix::AsyncFd;

        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            // nonblock=true sets O_NONBLOCK, required by AsyncFd.
            let ctl = match Ctl::new("default", true) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ewwkit: failed to open ALSA ctl: {e}");
                    return;
                }
            };
            if let Err(e) = ctl.subscribe_events(true) {
                eprintln!("ewwkit: failed to subscribe to ALSA events: {e}");
                return;
            }
            let async_ctl = match AsyncFd::new(ctl) {
                Ok(fd) => fd,
                Err(e) => {
                    eprintln!("ewwkit: failed to create AsyncFd for ALSA ctl: {e}");
                    return;
                }
            };

            loop {
                let mut guard = match async_ctl.readable().await {
                    Ok(g) => g,
                    Err(e) => {
                        eprintln!("ewwkit: ALSA poll error: {e}");
                        break;
                    }
                };
                guard.clear_ready();

                // Drain all pending events; only act when at least one VALUE event arrived.
                let mut value_changed = false;
                loop {
                    match async_ctl.get_ref().read_event() {
                        Ok(Some(ev)) if ev.get_mask() == EventMask::VALUE => {
                            value_changed = true;
                        }
                        Ok(Some(_)) => {} // add / remove / info — not a volume change
                        Ok(None) => break, // EAGAIN: no more events queued
                        Err(_) => break,
                    }
                }

                if value_changed {
                    match tokio::task::spawn_blocking(read_alsa_audio).await {
                        Ok(Ok(audio)) => {
                            if tx.send(audio).await.is_err() {
                                break;
                            }
                        }
                        Ok(Err(e)) => eprintln!("ewwkit: ALSA read error: {e}"),
                        Err(e) => eprintln!("ewwkit: spawn_blocking join error: {e}"),
                    }
                }
            }
        });

        rx
    }
}

#[cfg(feature = "alsa")]
fn read_alsa_audio() -> anyhow::Result<AudioState> {
    use alsa::mixer::{Mixer, SelemChannelId, SelemId};

    let mixer = Mixer::new("default", false)?;
    let selem_id = SelemId::new("Master", 0);
    let selem = mixer
        .find_selem(&selem_id)
        .ok_or_else(|| anyhow::anyhow!("ALSA: Master mixer element not found"))?;

    let (min, max) = selem.get_playback_volume_range();
    let raw = selem.get_playback_volume(SelemChannelId::FrontLeft)?;
    let volume = if max > min {
        ((raw - min) * 100 / (max - min)) as u8
    } else {
        0
    };
    let muted = selem.get_playback_switch(SelemChannelId::FrontLeft)? == 0;

    Ok(AudioState { volume, muted })
}

// ── Factory ──────────────────────────────────────────────────────────────────

#[cfg(not(feature = "alsa"))]
pub fn create_monitor() -> impl AudioProvider {
    PactlMonitor
}

#[cfg(feature = "alsa")]
pub fn create_monitor() -> impl AudioProvider {
    AlsaMonitor
}
