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
    KeepAlive,
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
            PopupAction::KeepAlive => {
                if let Some(popup) = &mut self.current_popup {
                    popup.opened_at = Instant::now();
                }
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
