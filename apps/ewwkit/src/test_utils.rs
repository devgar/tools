use crate::domain::{OutputState, WindowState, WorkspaceState};
use std::path::Path;

// ── Domain state builders ─────────────────────────────────────────────────────

pub fn focused_window() -> WindowState {
    WindowState { is_focused: true, ..Default::default() }
}

pub fn unfocused_window() -> WindowState {
    WindowState { is_focused: false, ..Default::default() }
}

pub fn workspace_with(windows: Vec<WindowState>) -> WorkspaceState {
    WorkspaceState { windows, ..Default::default() }
}

pub fn output_with(workspaces: Vec<WorkspaceState>) -> OutputState {
    OutputState { workspaces }
}

// ── Sysfs fixture helpers ─────────────────────────────────────────────────────

/// Creates a sysfs-like device directory `base/device/` populated with the
/// given `(filename, content)` pairs. Panics on any I/O error.
pub fn make_sysfs_files(base: &Path, device: &str, files: &[(&str, &str)]) {
    let dir = base.join(device);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
}

// ── Serde helpers ─────────────────────────────────────────────────────────────

/// Serializes `value` to JSON and deserializes it back. Panics on any error.
pub fn serde_roundtrip<T>(value: &T) -> T
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    let json = serde_json::to_string(value).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}
