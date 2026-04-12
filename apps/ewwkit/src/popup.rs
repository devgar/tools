use crate::domain::PopupState;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum PopupAction {
    Open {
        name: String,
        output: String,
        timeout: Option<Duration>,
    },
    Close,
}

pub struct PopupManager {
    current_popup: Option<InternalPopup>,
}

struct InternalPopup {
    name: String,
    output: String,
    opened_at: Instant,
    timeout: Option<Duration>,
    system_start_time: u64,
}

impl PopupManager {
    pub fn new() -> Self {
        Self {
            current_popup: None,
        }
    }

    pub fn handle_action(&mut self, action: PopupAction) {
        match action {
            PopupAction::Open {
                name,
                output,
                timeout,
            } => {
                let now = Instant::now();
                let system_now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                self.current_popup = Some(InternalPopup {
                    name,
                    output,
                    opened_at: now,
                    timeout,
                    system_start_time: system_now,
                });
            }
            PopupAction::Close => {
                self.current_popup = None;
            }
        }
    }

    pub fn check_timeouts(&mut self) {
        if let Some(popup) = &self.current_popup {
            if let Some(timeout) = popup.timeout {
                if popup.opened_at.elapsed() >= timeout {
                    self.current_popup = None;
                }
            }
        }
    }

    pub fn get_state(&self) -> Option<PopupState> {
        self.current_popup.as_ref().map(|p| PopupState {
            name: p.name.clone(),
            output: p.output.clone(),
            opened_at: p.system_start_time,
            timeout_ms: p.timeout.map(|d| d.as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str, output: &str, timeout: Option<Duration>) -> PopupAction {
        PopupAction::Open {
            name: name.to_string(),
            output: output.to_string(),
            timeout,
        }
    }

    #[test]
    fn new_manager_has_no_state() {
        assert!(PopupManager::new().get_state().is_none());
    }

    #[test]
    fn open_sets_name_output_and_timeout() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("volume", "HDMI-1", Some(Duration::from_millis(3000))));
        let s = pm.get_state().expect("popup must be set");
        assert_eq!(s.name, "volume");
        assert_eq!(s.output, "HDMI-1");
        assert_eq!(s.timeout_ms, Some(3000));
    }

    #[test]
    fn open_without_timeout_has_no_timeout_ms() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("dashboard", "DP-1", None));
        assert_eq!(pm.get_state().unwrap().timeout_ms, None);
    }

    #[test]
    fn open_replaces_existing_popup() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("volume", "HDMI-1", None));
        pm.handle_action(open("brightness", "DP-1", None));
        let s = pm.get_state().unwrap();
        assert_eq!(s.name, "brightness");
        assert_eq!(s.output, "DP-1");
    }

    #[test]
    fn close_clears_state() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("volume", "HDMI-1", None));
        pm.handle_action(PopupAction::Close);
        assert!(pm.get_state().is_none());
    }

    #[test]
    fn close_on_empty_manager_is_a_noop() {
        let mut pm = PopupManager::new();
        pm.handle_action(PopupAction::Close);
        assert!(pm.get_state().is_none());
    }

    #[test]
    fn check_timeouts_closes_expired_popup() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("volume", "HDMI-1", Some(Duration::ZERO)));
        pm.check_timeouts();
        assert!(pm.get_state().is_none());
    }

    #[test]
    fn check_timeouts_keeps_non_expired_popup() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("volume", "HDMI-1", Some(Duration::from_secs(60))));
        pm.check_timeouts();
        assert!(pm.get_state().is_some());
    }

    #[test]
    fn check_timeouts_never_closes_infinite_popup() {
        let mut pm = PopupManager::new();
        pm.handle_action(open("dashboard", "HDMI-1", None));
        pm.check_timeouts();
        assert!(pm.get_state().is_some());
    }
}
