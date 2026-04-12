use crate::config::AppConfig;
use crate::domain::{AppState, AudioState, BatteryState, BrightnessState, NetworkState, WindowManager};
use crate::infrastructure::controls;
use crate::infrastructure::ipc::{BrightnessAction, IpcMessage, VolumeAction};
use crate::infrastructure::niri::NiriAdapter;
use crate::popup::{PopupAction, PopupManager};

/// All events the daemon can receive, from any source.
pub enum AppEvent {
    BatteryChanged(BatteryState),
    NetworkChanged(NetworkState),
    AudioChanged(AudioState),
    BrightnessChanged(BrightnessState),
    /// Niri emitted a desktop-change signal; state is fetched inside the handler.
    NiriEvent,
    /// Popup timeout check tick (100 ms interval).
    PopupTick,
    Ipc(IpcMessage),
}

/// Returns the name of the output that contains the currently focused window,
fn focused_output(state: &AppState) -> Option<String> {
    state
        .focused_output()
        .or_else(|| state.desktop.outputs.keys().next().cloned())
}

/// Open an auto-dismissing popup on the focused output and sync `state.ui.popup`.
///
/// `name` is the popup widget name (e.g. `"volume"`, `"brightness"`).
fn trigger_popup(name: &str, state: &mut AppState, popup_manager: &mut PopupManager, config: &AppConfig) {
    let Some(output) = focused_output(state) else { return };
    popup_manager.handle_action(PopupAction::Open {
        name: name.to_string(),
        output,
        timeout: Some(config.popups.timeout),
    });
    state.ui.popup = popup_manager.get_state();
}

/// Process one event, mutate state in place, return true if state changed.
pub async fn handle_event(
    event: AppEvent,
    state: &mut AppState,
    popup_manager: &mut PopupManager,
    config: &AppConfig,
    niri_adapter: &NiriAdapter,
) -> bool {
    match event {
        AppEvent::BatteryChanged(bat) => {
            if state.system.battery != bat {
                state.system.battery = bat;
                return true;
            }
        }

        AppEvent::NetworkChanged(net) => {
            if state.system.network != net {
                state.system.network = net;
                return true;
            }
        }

        AppEvent::AudioChanged(audio) => {
            if state.system.audio != audio {
                state.system.audio = audio;
                trigger_popup("volume", state, popup_manager, config);
                return true;
            }
        }

        AppEvent::BrightnessChanged(brightness) => {
            if state.system.brightness != brightness {
                state.system.brightness = brightness;
                trigger_popup("brightness", state, popup_manager, config);
                return true;
            }
        }

        AppEvent::NiriEvent => {
            if let Ok(desktop) = niri_adapter.get_desktop_state().await {
                if state.desktop.outputs != desktop.outputs {
                    state.desktop.outputs = desktop.outputs;
                    return true;
                }
            }
        }

        AppEvent::PopupTick => {
            let old = popup_manager.get_state();
            popup_manager.check_timeouts();
            let new = popup_manager.get_state();
            if old != new {
                state.ui.popup = new;
                return true;
            }
        }

        AppEvent::Ipc(msg) => match msg {
            IpcMessage::Popup { name, output, keep } => {
                let Some(output) = output.or_else(|| focused_output(state)) else { return false };
                let timeout = (!keep).then_some(config.popups.timeout);
                popup_manager.handle_action(PopupAction::Open { name, output, timeout });
                state.ui.popup = popup_manager.get_state();
                return true;
            }
            IpcMessage::ClosePopup => {
                popup_manager.handle_action(PopupAction::Close);
                state.ui.popup = popup_manager.get_state();
                return true;
            }
            IpcMessage::Volume(action) => {
                let result = match action {
                    VolumeAction::Up { step } => controls::volume_up(step).await,
                    VolumeAction::Down { step } => controls::volume_down(step).await,
                    VolumeAction::Set { percent } => controls::volume_set(percent).await,
                    VolumeAction::Mute => controls::volume_mute_toggle().await,
                };
                if let Err(e) = result {
                    eprintln!("ewwkit: volume control error: {e}");
                }
                // state_changed = false — audio_rx will pick up the change
            }
            IpcMessage::Brightness(action) => {
                let result = match action {
                    BrightnessAction::Up { step } => controls::brightness_up(step).await,
                    BrightnessAction::Down { step } => controls::brightness_down(step).await,
                    BrightnessAction::Set { percent } => controls::brightness_set(percent).await,
                };
                if let Err(e) = result {
                    eprintln!("ewwkit: brightness control error: {e}");
                }
                // state_changed = false
            }
            IpcMessage::GetState => {}
        },
    }

    false
}
