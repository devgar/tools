use crate::domain::{WindowManager, Workspace, Window};
use async_trait::async_trait;
use serde_json::Value;
use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader, Write};
use std::env;
use tokio::io::AsyncBufReadExt;

pub struct NiriAdapter {
    socket_path: String,
}

impl NiriAdapter {
    pub fn new(config_socket_path: &Option<String>) -> Self {
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

        
        Self { socket_path }
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
    async fn get_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        let ws_json = self.send_request("Workspaces")?;
        let ws_array = ws_json.as_array().ok_or_else(|| anyhow::anyhow!("Expected array for Workspaces, got: {}", ws_json))?;

        let win_json = self.send_request("Windows")?;
        let default_wins = vec![];
        let win_array = win_json.as_array().unwrap_or(&default_wins);

        let workspaces = ws_array.iter().map(|w| {
            let internal_id = w["id"].as_u64().unwrap_or(0);
            let idx = w["idx"].as_u64().unwrap_or(0) as u32;
            let active = w["is_active"].as_bool().unwrap_or(false);
            let output_name = w["output"].as_str().unwrap_or("").to_string();
            
            let count = win_array.iter()
                .filter(|win| win["workspace_id"].as_u64() == Some(internal_id))
                .count() as u32;

            Workspace {
                id: idx,
                active,
                windows_count: count,
                output: output_name,
            }
        }).collect();

        Ok(workspaces)
    }

    async fn get_windows(&self) -> anyhow::Result<Vec<Window>> {
        let win_json = self.send_request("Windows")?;
        let win_array = win_json.as_array().ok_or_else(|| anyhow::anyhow!("Expected array for Windows, got: {}", win_json))?;

        let windows = win_array.iter().map(|w| {
            Window {
                id: w["id"].as_u64().unwrap_or(0),
                title: w["title"].as_str().unwrap_or("").to_string(),
                app_id: w["app_id"].as_str().map(|s| s.to_string()),
                workspace_id: w["workspace_id"].as_u64().unwrap_or(0),
                is_focused: w["is_focused"].as_bool().unwrap_or(false),
            }
        }).collect();

        Ok(windows)
    }

    async fn get_focused_window_id(&self) -> anyhow::Result<Option<u64>> {
        let win_json = self.send_request("FocusedWindow")?;
        
        // Si FocusedWindow devuelve null (ninguna ventana enfocada)
        if win_json.is_null() {
            return Ok(None);
        }

        // Si devuelve el objeto de la ventana directamente
        if let Some(id) = win_json["id"].as_u64() {
            return Ok(Some(id));
        }

        Ok(None)
    }
}
