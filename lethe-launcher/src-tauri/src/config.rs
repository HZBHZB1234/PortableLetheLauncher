use std::collections::HashMap;
use std::fs;
use std::path::Path;

const CONFIG_PATH: &str = "lethe-launcher.ini";

fn parse_ini_text(contents: &str, config: &mut HashMap<String, String>) {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(sep) = trimmed.find('=') {
            let key = trimmed[..sep].trim().to_string();
            let value = trimmed[sep + 1..].trim().to_string();
            config.insert(key, value);
        }
    }
}

pub fn read_config() -> HashMap<String, String> {
    let mut config = HashMap::new();
    if let Ok(contents) = fs::read_to_string(CONFIG_PATH) {
        parse_ini_text(&contents, &mut config);
    }
    config
}

/// Settings for the currently active profile. The root file is read first so
/// older installs keep their settings; the profile file wins on conflicts.
pub fn read_active_config() -> HashMap<String, String> {
    let mut config = read_config();
    let path = crate::profiles::active_ini_path();
    if let Ok(contents) = fs::read_to_string(path) {
        parse_ini_text(&contents, &mut config);
    }
    config
}

pub fn get_config_value(key: &str, default: &str) -> String {
    let config = read_active_config();
    config
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

pub fn is_auto_update_disabled() -> bool {
    get_config_value("DisableAutoUpdate", "false").to_lowercase() == "true"
}

pub fn create_default_config() {
    if !Path::new(CONFIG_PATH).exists() {
        let _ = fs::write(CONFIG_PATH, "DisableAutoUpdate=false\n");
    }
}

fn set_in_file(path: &Path, key: &str, value: &str) {
    let mut config = HashMap::new();
    if let Ok(contents) = fs::read_to_string(path) {
        parse_ini_text(&contents, &mut config);
    }
    config.insert(key.to_string(), value.to_string());
    let mut content = String::new();
    for (k, v) in &config {
        content.push_str(&format!("{}={}\n", k, v));
    }
    let _ = fs::write(path, content);
}

/// Write a setting to both the profile ini and the root compatibility file.
pub fn set_config_value(key: &str, value: &str) {
    set_in_file(Path::new(CONFIG_PATH), key, value);
    set_in_file(&crate::profiles::active_ini_path(), key, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_config_disabled() {
        fs::write("test-config.ini", "DisableAutoUpdate=true\n").unwrap();
        let mut map = HashMap::new();
        parse_ini_text(&fs::read_to_string("test-config.ini").unwrap(), &mut map);
        assert_eq!(
            map.get("DisableAutoUpdate").map(String::as_str),
            Some("true")
        );
        fs::remove_file("test-config.ini").unwrap();
    }

    #[test]
    fn test_get_config_value_default() {
        let val = get_config_value("NonExistent", "default_val");
        assert_eq!(val, "default_val");
    }
}
