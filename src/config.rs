use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_TOML: &str = r#"# waytime configuration

# List of application IDs to ignore from screen time tracking.
# Matching is case-insensitive.
# Example: ignore_apps = ["swaylock", "hyprlock"]
ignore_apps = []
"#;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub ignore_apps: HashSet<String>,
}

impl Config {
    pub fn resolve_path(custom_path: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(path) = custom_path {
            Ok(path.to_path_buf())
        } else {
            let proj_dirs = directories::ProjectDirs::from("", "", "waytime")
                .ok_or("Could not determine configuration directory")?;
            Ok(proj_dirs.config_dir().join("config.toml"))
        }
    }

    pub fn load(custom_path: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = Self::resolve_path(custom_path)?;

        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, DEFAULT_CONFIG_TOML)?;
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn is_ignored(&self, app_id: &str) -> bool {
        self.ignore_apps.iter().any(|ignored| {
            ignored.eq_ignore_ascii_case(app_id) || ignored.to_lowercase() == app_id.to_lowercase()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_project_dirs_path() {
        let path = Config::resolve_path(None).unwrap();
        assert!(path.ends_with("waytime/config.toml"));
    }

    #[test]
    fn test_resolve_custom_path() {
        let custom = Path::new("/tmp/custom_waytime.toml");
        let path = Config::resolve_path(Some(custom)).unwrap();
        assert_eq!(path, custom);
    }   

    #[test]
    fn test_is_ignored_case_insensitive() {
        let mut config = Config::default();
        config.ignore_apps.insert("Firefox".to_string());
        config.ignore_apps.insert("Code".to_string());

        assert!(config.is_ignored("Firefox"));
        assert!(config.is_ignored("firefox"));
        assert!(config.is_ignored("FIREFOX"));
        assert!(config.is_ignored("fIrEfOx"));

        assert!(config.is_ignored("Code"));
        assert!(config.is_ignored("code"));
        assert!(config.is_ignored("CODE"));

        assert!(!config.is_ignored("chrome"));
        assert!(!config.is_ignored(""));
        assert!(!config.is_ignored("Fire"));
    }

    #[test]
    fn test_load_creates_default_when_missing() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_dir = std::env::temp_dir().join(format!("waytime_test_{nonce}"));
        let test_config_path = test_dir.join("sub").join("config.toml");

        assert!(!test_config_path.exists());

        // First load should create directories, write default config, and return default
        let config = Config::load(Some(&test_config_path)).expect("failed to load missing config");
        assert_eq!(config, Config::default());
        assert!(test_config_path.exists());

        let content = fs::read_to_string(&test_config_path).expect("failed to read created file");
        assert_eq!(content, DEFAULT_CONFIG_TOML);

        // Modify the file
        fs::write(
            &test_config_path,
            r#"ignore_apps = ["custom_app", "another_app"]"#,
        )
        .expect("failed to overwrite config");

        // Second load should read modified file
        let loaded = Config::load(Some(&test_config_path)).expect("failed to reload config");
        assert_eq!(loaded.ignore_apps.len(), 2);
        assert!(loaded.is_ignored("custom_app"));
        assert!(loaded.is_ignored("CUSTOM_APP"));
        assert!(loaded.is_ignored("another_app"));

        let _ = fs::remove_dir_all(&test_dir);
    }
}
