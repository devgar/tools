use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::RwLock;

pub struct IconResolver {
    cache: RwLock<HashMap<String, String>>,
    default_icon: String,
}

impl IconResolver {
    pub fn new(_custom_icon_dir: &str) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            default_icon: "/usr/share/icons/hicolor/48x48/apps/icon-missing.png".to_string(),
        }
    }

    pub fn resolve(&self, app_id: &Option<String>) -> String {
        let app_id = match app_id {
            Some(id) if !id.is_empty() => id,
            _ => return self.default_icon.clone(),
        };

        // Check cache first
        {
            let cache = self.cache.read().unwrap();
            if let Some(path) = cache.get(app_id) {
                return path.clone();
            }
        }

        // 1. Intentar con freedesktop-icons directamente
        let mut resolved = freedesktop_icons::lookup(app_id)
            .with_size(48)
            .find()
            .map(|p| p.to_string_lossy().to_string());

        // 2. Si falla, intentar buscar el .desktop para extraer el campo Icon (como el script de python)
        if resolved.is_none() {
            resolved = self.extract_from_desktop(app_id);
        }

        let final_path = resolved.unwrap_or_else(|| self.default_icon.clone());

        // Update cache
        let mut cache = self.cache.write().unwrap();
        cache.insert(app_id.clone(), final_path.clone());

        final_path
    }

    fn extract_from_desktop(&self, app_id: &str) -> Option<String> {
        let paths = [
            "/usr/share/applications",
            &format!(
                "{}/.local/share/applications",
                std::env::var("HOME").unwrap_or_default()
            ),
        ];

        for base_path in &paths {
            let desktop_file = format!("{}/{}.desktop", base_path, app_id);
            if let Ok(content) = fs::read_to_string(&desktop_file) {
                for line in content.lines() {
                    if line.starts_with("Icon=") {
                        let icon_name = line.split('=').nth(1)?.trim();
                        if Path::new(icon_name).is_absolute() {
                            return Some(icon_name.to_string());
                        }
                        // Volvemos a intentar buscar el nombre extraído
                        return freedesktop_icons::lookup(icon_name)
                            .with_size(48)
                            .find()
                            .map(|p| p.to_string_lossy().to_string());
                    }
                }
            }
        }
        None
    }
}
