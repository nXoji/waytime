use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_TOML: &str = r#"# waytime configuration

# List of application IDs to ignore from screen time tracking.
# Matching is case-insensitive.
# Example: ignore_apps = ["swaylock", "hyprlock"]
ignore_apps = []

# Application categories mapping category name to list of application IDs.
# Matching is case-insensitive.
# Example:
# [categories]
# Development = ["code", "nvim"]

# Daily time limits for categories or specific applications.
# Formats support units: d, h, m, s (e.g. "2h", "45m", "1h 30m").
# Example:
# [limits]
# Development = "2h"
"#;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub ignore_apps: HashSet<String>,
    #[serde(default)]
    pub categories: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub limits: HashMap<String, String>,
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

    pub fn get_category(&self, app_id: &str) -> Option<&str> {
        for (category, apps) in &self.categories {
            if apps.iter().any(|app| {
                app.eq_ignore_ascii_case(app_id) || app.to_lowercase() == app_id.to_lowercase()
            }) {
                return Some(category.as_str());
            }
        }
        None
    }

    pub fn parse_limit_seconds(val: &str) -> Option<u64> {
        let s = val.trim();
        if s.is_empty() {
            return None;
        }

        let mut total_seconds: u64 = 0;
        let mut chars = s.chars().peekable();
        let mut parsed_any = false;

        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }

            if !c.is_ascii_digit() {
                return None;
            }

            let mut num: u64 = 0;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    chars.next();
                    let digit = d.to_digit(10)? as u64;
                    num = num.checked_mul(10)?.checked_add(digit)?;
                } else {
                    break;
                }
            }

            while let Some(&w) = chars.peek() {
                if w.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }

            let unit_char = chars.next()?;
            let multiplier: u64 = match unit_char.to_ascii_lowercase() {
                'd' => 86_400,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => return None,
            };

            let unit_seconds = num.checked_mul(multiplier)?;
            total_seconds = total_seconds.checked_add(unit_seconds)?;
            parsed_any = true;
        }

        if parsed_any {
            Some(total_seconds)
        } else {
            None
        }
    }

    pub fn get_limit_seconds(&self, target: &str) -> Option<u64> {
        let find_limit = |key: &str| -> Option<&str> {
            if let Some(val) = self.limits.get(key) {
                return Some(val.as_str());
            }
            self.limits.iter().find_map(|(k, v)| {
                if k.eq_ignore_ascii_case(key) || k.to_lowercase() == key.to_lowercase() {
                    Some(v.as_str())
                } else {
                    None
                }
            })
        };

        if let Some(raw_limit) = find_limit(target) {
            return Self::parse_limit_seconds(raw_limit);
        }

        if let Some(raw_limit) = self.get_category(target).and_then(find_limit) {
            return Self::parse_limit_seconds(raw_limit);
        }

        None
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

    #[test]
    fn test_category_lookup_case_insensitive() {
        let mut config = Config::default();
        config.categories.insert(
            "Development".to_string(),
            vec![
                "code".to_string(),
                "nvim".to_string(),
                "RustRover".to_string(),
            ],
        );
        config.categories.insert(
            "Social".to_string(),
            vec!["Discord".to_string(), "Slack".to_string()],
        );

        // Matching development apps with different casings
        assert_eq!(config.get_category("code"), Some("Development"));
        assert_eq!(config.get_category("CODE"), Some("Development"));
        assert_eq!(config.get_category("Code"), Some("Development"));
        assert_eq!(config.get_category("nvim"), Some("Development"));
        assert_eq!(config.get_category("NVIM"), Some("Development"));
        assert_eq!(config.get_category("rustrover"), Some("Development"));
        assert_eq!(config.get_category("RUSTROVER"), Some("Development"));

        // Matching social apps with different casings
        assert_eq!(config.get_category("discord"), Some("Social"));
        assert_eq!(config.get_category("DISCORD"), Some("Social"));
        assert_eq!(config.get_category("Discord"), Some("Social"));
        assert_eq!(config.get_category("slack"), Some("Social"));
        assert_eq!(config.get_category("SLACK"), Some("Social"));

        // Non-existent apps and empty strings
        assert_eq!(config.get_category("firefox"), None);
        assert_eq!(config.get_category(""), None);
        assert_eq!(config.get_category("dev"), None);
    }

    #[test]
    fn test_parse_limit_seconds_valid_and_invalid() {
        // Valid formats
        assert_eq!(Config::parse_limit_seconds("2h"), Some(7200));
        assert_eq!(Config::parse_limit_seconds("45m"), Some(2700));
        assert_eq!(Config::parse_limit_seconds("1h 30m"), Some(5400));
        assert_eq!(Config::parse_limit_seconds("1h30m"), Some(5400));
        assert_eq!(Config::parse_limit_seconds("1d"), Some(86400));
        assert_eq!(Config::parse_limit_seconds("10s"), Some(10));
        assert_eq!(Config::parse_limit_seconds("0s"), Some(0));
        assert_eq!(Config::parse_limit_seconds("0m"), Some(0));
        assert_eq!(
            Config::parse_limit_seconds("1d 2h 3m 4s"),
            Some(86400 + 7200 + 180 + 4)
        );
        assert_eq!(Config::parse_limit_seconds(" 2h "), Some(7200));
        assert_eq!(Config::parse_limit_seconds("1H 30M"), Some(5400));
        assert_eq!(Config::parse_limit_seconds("1 h 30 m"), Some(5400));
        assert_eq!(Config::parse_limit_seconds(" 45 m "), Some(2700));

        // Invalid formats
        assert_eq!(Config::parse_limit_seconds(""), None);
        assert_eq!(Config::parse_limit_seconds("   "), None);
        assert_eq!(Config::parse_limit_seconds("abc"), None);
        assert_eq!(Config::parse_limit_seconds("10"), None);
        assert_eq!(Config::parse_limit_seconds("10x"), None);
        assert_eq!(Config::parse_limit_seconds("-5m"), None);
        assert_eq!(Config::parse_limit_seconds("1h -30m"), None);
        assert_eq!(Config::parse_limit_seconds("h"), None);
        assert_eq!(Config::parse_limit_seconds("1.5h"), None);
        assert_eq!(Config::parse_limit_seconds("1h 2"), None);
        assert_eq!(Config::parse_limit_seconds("1h foo"), None);
        assert_eq!(Config::parse_limit_seconds("10ms"), None);
    }

    #[test]
    fn test_get_limit_seconds() {
        let mut config = Config::default();
        config.categories.insert(
            "Development".to_string(),
            vec!["code".to_string(), "nvim".to_string()],
        );
        config
            .categories
            .insert("Social".to_string(), vec!["discord".to_string()]);

        config
            .limits
            .insert("Development".to_string(), "2h".to_string());
        config.limits.insert("code".to_string(), "45m".to_string());
        config
            .limits
            .insert("Social".to_string(), "1h 30m".to_string());

        // Category direct limit lookup (case-insensitive)
        assert_eq!(config.get_limit_seconds("Development"), Some(7200));
        assert_eq!(config.get_limit_seconds("development"), Some(7200));
        assert_eq!(config.get_limit_seconds("DEVELOPMENT"), Some(7200));

        // App-specific limit override (code has 45m instead of category's 2h)
        assert_eq!(config.get_limit_seconds("code"), Some(2700));
        assert_eq!(config.get_limit_seconds("CODE"), Some(2700));

        // Category limit inherited by app without specific limit (nvim inherits Development limit)
        assert_eq!(config.get_limit_seconds("nvim"), Some(7200));
        assert_eq!(config.get_limit_seconds("NVIM"), Some(7200));

        // Social app inherits Social category limit
        assert_eq!(config.get_limit_seconds("discord"), Some(5400));
        assert_eq!(config.get_limit_seconds("DISCORD"), Some(5400));

        // Unconfigured target
        assert_eq!(config.get_limit_seconds("firefox"), None);
        assert_eq!(config.get_limit_seconds(""), None);
    }

    #[test]
    fn test_config_serialization_deserialization() {
        let mut config = Config::default();
        config.ignore_apps.insert("swaylock".to_string());
        config.categories.insert(
            "Development".to_string(),
            vec!["code".to_string(), "nvim".to_string()],
        );
        config
            .categories
            .insert("Social".to_string(), vec!["discord".to_string()]);
        config
            .limits
            .insert("Development".to_string(), "2h".to_string());
        config
            .limits
            .insert("discord".to_string(), "30m".to_string());

        // Serialize to TOML
        let toml_str = toml::to_string(&config).expect("failed to serialize config");

        // Deserialize back from TOML
        let deserialized: Config = toml::from_str(&toml_str).expect("failed to deserialize config");
        assert_eq!(deserialized, config);

        // Deserialize from handwritten TOML with categories and limits
        let toml_input = r#"
ignore_apps = ["hyprlock"]

[categories]
Development = ["code", "nvim"]
Gaming = ["steam"]

[limits]
Development = "4h"
Gaming = "1h 30m"
steam = "45m"
"#;
        let loaded: Config = toml::from_str(toml_input).expect("failed to parse TOML");
        assert!(loaded.is_ignored("hyprlock"));
        assert_eq!(loaded.get_category("code"), Some("Development"));
        assert_eq!(loaded.get_category("STEAM"), Some("Gaming"));
        assert_eq!(loaded.get_limit_seconds("Development"), Some(14400));
        assert_eq!(loaded.get_limit_seconds("nvim"), Some(14400));
        assert_eq!(loaded.get_limit_seconds("steam"), Some(2700));
        assert_eq!(loaded.get_limit_seconds("Gaming"), Some(5400));

        // Deserializing empty TOML defaults categories and limits to empty maps
        let empty_loaded: Config = toml::from_str("").expect("failed to parse empty TOML");
        assert_eq!(empty_loaded, Config::default());
        assert!(empty_loaded.categories.is_empty());
        assert!(empty_loaded.limits.is_empty());
    }
}
