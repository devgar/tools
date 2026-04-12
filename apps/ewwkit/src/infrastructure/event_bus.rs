use crate::application::AppEvent;
use crate::domain::{AudioState, BatteryState, BrightnessState, NetworkState};
use crate::infrastructure::ipc::IpcServer;
use tokio::sync::mpsc;
use tokio::time::{self, Duration, MissedTickBehavior};

pub struct EventBus {
    battery_rx: mpsc::Receiver<BatteryState>,
    audio_rx: mpsc::Receiver<AudioState>,
    wifi_rx: mpsc::Receiver<NetworkState>,
    brightness_rx: mpsc::Receiver<BrightnessState>,
    niri_events: mpsc::Receiver<()>,
    popup_check: time::Interval,
    ipc_server: IpcServer,
}

impl EventBus {
    pub fn new(
        battery_rx: mpsc::Receiver<BatteryState>,
        audio_rx: mpsc::Receiver<AudioState>,
        wifi_rx: mpsc::Receiver<NetworkState>,
        brightness_rx: mpsc::Receiver<BrightnessState>,
        niri_events: mpsc::Receiver<()>,
        ipc_server: IpcServer,
    ) -> Self {
        let mut popup_check = time::interval(Duration::from_millis(100));
        // Skip missed ticks if the daemon is momentarily slow — no burst replay.
        popup_check.set_missed_tick_behavior(MissedTickBehavior::Skip);

        Self {
            battery_rx,
            audio_rx,
            wifi_rx,
            brightness_rx,
            niri_events,
            popup_check,
            ipc_server,
        }
    }

    /// Returns the next event from any source. Never returns `None`.
    pub async fn next(&mut self) -> AppEvent {
        loop {
            tokio::select! {
                Some(bat) = self.battery_rx.recv() => return AppEvent::BatteryChanged(bat),
                Some(audio) = self.audio_rx.recv() => return AppEvent::AudioChanged(audio),
                Some(net) = self.wifi_rx.recv() => return AppEvent::NetworkChanged(net),
                Some(b) = self.brightness_rx.recv() => return AppEvent::BrightnessChanged(b),
                Some(_) = self.niri_events.recv() => return AppEvent::NiriEvent,
                _ = self.popup_check.tick() => return AppEvent::PopupTick,
                msg = self.ipc_server.accept_message() => {
                    if let Some(msg) = msg {
                        return AppEvent::Ipc(msg);
                    }
                    // None = connection error or deserialisation failure — retry.
                }
            }
        }
    }
}
