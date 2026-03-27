use crate::domain::{DesktopState, OutputState, WindowManager, WindowState, WorkspaceState};
use crate::infrastructure::icons::IconResolver;
use async_trait::async_trait;
use serde_json::Value;
use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader, Write};
use std::env;
use tokio::io::AsyncBufReadExt;
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub struct NiriAdapter {
    socket_path: String,
    icon_resolver: IconResolver,
}

impl NiriAdapter {
    pub fn new(config_socket_path: &Option<String>, icon_dir: &str) -> Self {
        let socket_path = config_socket_path.clone().unwrap_or_else(|| {
            match env::var("NIRI_SOCKET") {
                Ok(path) => path,
                Err(_) => {
                    let xdg_runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
                    // Buscar un socket que empiece por "niri" en XDG_RUNTIME_DIR
                    std::fs::read_dir(&xdg_runtime)
                        .map(|entries| {
                            entries.filter_map(|e| e.ok())
                                .map(|e| e.file_name().into_string().unwrap_or_default())
                                .find(|name| name.starts_with("niri") && name.ends_with(".sock"))
                                .map(|name| format!("{}/{}", xdg_runtime, name))
                        })
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| format!("{}/niri-0", xdg_runtime))
                }
            }
        });

        
        Self { 
            socket_path,
            icon_resolver: IconResolver::new(icon_dir),
        }
    }

    fn send_request(&self, request: &str) -> anyhow::Result<Value> {
        let stream = UnixStream::connect(&self.socket_path);
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => return Err(anyhow::anyhow!("Failed to connect to niri socket at {}: {}", self.socket_path, e)),
        };
        // Según las pruebas con socat, niri espera el string del comando directamente como JSON con un newline
        let payload = format!("\"{}\"\n", request); 
        stream.write_all(payload.as_bytes())?;
        stream.flush()?;
        
        let reader = BufReader::new(stream);
        if let Some(line) = reader.lines().next() {
            let line = line?;
            let val: Value = serde_json::from_str(&line)?;
            
            // Niri envuelve la respuesta en {"Ok": ...} o directamente en un objeto con el nombre del comando
            let val = if let Some(ok_val) = val.get("Ok") {
                ok_val.clone()
            } else {
                val
            };

            // Niri envuelve la data en un objeto con la llave del comando (ej: {"Windows": [...]})
            if let Some(wrapped_val) = val.get(request) {
                return Ok(wrapped_val.clone());
            }
            
            Ok(val)
        } else {
            Err(anyhow::anyhow!("No response from niri socket"))
        }
    }

    pub async fn event_listener(socket_path: String) -> anyhow::Result<tokio::sync::mpsc::Receiver<()>> {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        
        tokio::spawn(async move {
            if let Ok(stream) = tokio::net::UnixStream::connect(&socket_path).await {
                let (reader, mut writer) = tokio::io::split(stream);
                use tokio::io::AsyncWriteExt;
                
                // Enviar comando para iniciar el stream de eventos sin corchetes y con newline
                if let Err(e) = writer.write_all(b"\"EventStream\"\n").await {
                    eprintln!("Failed to write to niri socket: {}", e);
                    return;
                }
                let _ = writer.flush().await;

                let mut lines = tokio::io::BufReader::new(reader).lines();
                while let Ok(Some(_)) = lines.next_line().await {
                    let _ = tx.send(()).await;
                }
            } else {
                eprintln!("Failed to connect to niri socket for events at {}", socket_path);
            }
        });

        Ok(rx)
    }
}

#[async_trait]
impl WindowManager for NiriAdapter {
    async fn get_desktop_state(&self) -> anyhow::Result<DesktopState> {
        let ws_json = self.send_request("Workspaces")?;
        let ws_array = ws_json.as_array().ok_or_else(|| anyhow::anyhow!("Expected array for Workspaces"))?;

        let win_json = self.send_request("Windows")?;
        let win_array = win_json.as_array().ok_or_else(|| anyhow::anyhow!("Expected array for Windows"))?;

        // 1. Parse and sort all windows
        let mut all_windows = win_array.iter().map(|w| {
            let app_id = w["app_id"].as_str().map(|s| s.to_string());
            let app_icon = self.icon_resolver.resolve(&app_id);
            
            (
                WindowState {
                    id: w["id"].as_u64().unwrap_or(0),
                    title: w["title"].as_str().unwrap_or("").to_string(),
                    app_id,
                    is_focused: w["is_focused"].as_bool().unwrap_or(false),
                    app_icon,
                },
                w["workspace_id"].as_u64().unwrap_or(0),
                w["layout"]["pos_in_scrolling_layout"].as_array().map(|a| {
                    (a[0].as_i64().unwrap_or(0), a[1].as_i64().unwrap_or(0))
                }),
                w["layout"]["tile_pos_in_workspace_view"].as_array().map(|a| {
                    (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0))
                })
            )
        }).collect::<Vec<_>>();

        // Sort by layout priority
        all_windows.sort_by(|a, b| {
            match (&a.2, &b.2) {
                (Some(pos_a), Some(pos_b)) => pos_a.cmp(pos_b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => {
                    match (&a.3, &b.3) {
                        (Some(pos_a), Some(pos_b)) => {
                            pos_a.0.partial_cmp(&pos_b.0).unwrap_or(Ordering::Equal)
                                .then(pos_a.1.partial_cmp(&pos_b.1).unwrap_or(Ordering::Equal))
                        },
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    }
                }
            }
        });

        // 2. Group workspaces by output
        let mut outputs_map: BTreeMap<String, Vec<WorkspaceState>> = BTreeMap::new();

        for ws in ws_array {
            let id = ws["id"].as_u64().unwrap_or(0);
            let idx = ws["idx"].as_u64().unwrap_or(0) as u32;
            let name = ws["name"].as_str().map(String::from);
            let active = ws["is_active"].as_bool().unwrap_or(false);
            let output_name = ws["output"].as_str().unwrap_or("").to_string();

            let ws_windows = all_windows.iter()
                .filter(|(_, ws_id, _, _)| *ws_id == id)
                .map(|(win, _, _, _)| win.clone())
                .collect::<Vec<WindowState>>();

            let workspace = WorkspaceState {
                id,
                idx,
                name,
                active,
                windows: ws_windows,
            };

            outputs_map.entry(output_name).or_insert_with(Vec::new).push(workspace);
        }

        for workspaces in outputs_map.values_mut() {
            workspaces.sort_by_key(|w| w.idx);
        }

        let outputs = outputs_map
            .into_iter()
            .map(|(name, workspaces)| {
                (name, OutputState { workspaces })
            })
            .collect::<BTreeMap<String, OutputState>>();

        Ok(DesktopState { outputs })
    }
}
