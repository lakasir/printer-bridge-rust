use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub(crate) const DEFAULT_PAPER_WIDTH: u16 = 48;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) printer_name: String,
    pub(crate) paper_width: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            printer_name: String::new(),
            paper_width: DEFAULT_PAPER_WIDTH,
        }
    }
}

fn settings_path() -> Result<PathBuf, String> {
    let config_dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }
    .ok_or_else(|| "Could not determine the user configuration directory".to_string())?;

    Ok(config_dir
        .join("lakasir")
        .join("printer-bridge")
        .join("settings.json"))
}

pub(crate) fn load_settings() -> Result<Settings, String> {
    let path = settings_path()?;
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("Could not read settings: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Could not read settings: {error}")),
    }
}

pub(crate) fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "Could not determine the settings directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create settings directory: {error}"))?;
    let contents = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("Could not encode settings: {error}"))?;
    fs::write(path, contents).map_err(|error| format!("Could not save settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_use_the_existing_defaults_and_json_field_names() {
        let settings = Settings::default();
        assert_eq!(settings.printer_name, "");
        assert_eq!(settings.paper_width, 48);
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            r#"{"printerName":"","paperWidth":48}"#
        );
    }
}
