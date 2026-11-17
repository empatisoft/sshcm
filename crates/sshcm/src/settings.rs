//! sshcm's own settings in `~/.config/sshcm/config.toml`. Connections are
//! never stored here; they live only in the ssh config file.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::i18n::{Lang, tr, trf};

pub const KEYS: [&str; 3] = ["language", "backup_keep", "theme"];

/// Values of the `theme` setting.
const THEMES: [&str; 3] = ["auto", "dark", "light"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub backup_keep: usize,
    /// Colors of the full-screen interface: `auto` asks the terminal for
    /// its background, `dark` and `light` skip the question.
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "en".to_string(),
            backup_keep: 10,
            theme: "auto".to_string(),
        }
    }
}

impl Settings {
    /// `$XDG_CONFIG_HOME/sshcm/config.toml`, or `~/.config/sshcm/config.toml`.
    pub fn path(home: &Path) -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        base.join("sshcm").join("config.toml")
    }

    /// Reads the settings file. A missing file gives the defaults.
    pub fn load(home: &Path) -> Result<Settings> {
        let path = Settings::path(home);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(AppError::Message(format!("{}: {e}", path.display()))),
        };
        let settings: Settings = toml::from_str(&text)
            .map_err(|e| AppError::Message(format!("{}: {}", path.display(), e.message())))?;
        settings.check(&path)?;
        Ok(settings)
    }

    fn check(&self, path: &Path) -> Result<()> {
        if Lang::parse(&self.language).is_none()
            || self.backup_keep == 0
            || !THEMES.contains(&self.theme.as_str())
        {
            return Err(AppError::Message(trf(
                "{} has an invalid value. Fix it with: sshcm config set <key> <value>",
                "{} geçersiz bir değer içeriyor. Düzeltmek için: sshcm config set <ayar> <değer>",
                &[&path.display().to_string()],
            )));
        }
        Ok(())
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        let path = Settings::path(home);
        let io = |e: std::io::Error| AppError::Message(format!("{}: {e}", path.display()));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io)?;
        }
        let text = toml::to_string(self).map_err(|e| AppError::Message(e.to_string()))?;
        fs::write(&path, text).map_err(io)
    }

    pub fn lang(&self) -> Lang {
        Lang::parse(&self.language).unwrap_or(Lang::En)
    }

    pub fn get(&self, key: &str) -> Result<String> {
        match key {
            "language" => Ok(self.language.clone()),
            "backup_keep" => Ok(self.backup_keep.to_string()),
            "theme" => Ok(self.theme.clone()),
            _ => Err(unknown_key(key)),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "language" => {
                let lang = Lang::parse(value).ok_or_else(|| {
                    AppError::msg(tr(
                        "language must be en or tr.",
                        "language değeri en veya tr olmalıdır.",
                    ))
                })?;
                self.language = lang.code().to_string();
            }
            "backup_keep" => {
                self.backup_keep = value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| {
                        AppError::msg(tr(
                            "backup_keep must be a whole number, 1 or more.",
                            "backup_keep 1 veya daha büyük bir tam sayı olmalıdır.",
                        ))
                    })?;
            }
            "theme" => {
                let theme = value.trim().to_ascii_lowercase();
                if !THEMES.contains(&theme.as_str()) {
                    return Err(AppError::msg(tr(
                        "theme must be auto, dark or light.",
                        "theme değeri auto, dark veya light olmalıdır.",
                    )));
                }
                self.theme = theme;
            }
            _ => return Err(unknown_key(key)),
        }
        Ok(())
    }
}

fn unknown_key(key: &str) -> AppError {
    AppError::Message(trf(
        "Unknown setting: {}. Available settings: {}",
        "Bilinmeyen ayar: {}. Kullanılabilir ayarlar: {}",
        &[key, &KEYS.join(", ")],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_files() {
        assert_eq!(Settings::default().backup_keep, 10);
        let s: Settings = toml::from_str("language = \"tr\"\n").unwrap();
        assert_eq!(s.lang(), Lang::Tr);
        assert_eq!(s.backup_keep, 10);
    }

    #[test]
    fn set_validates_values() {
        let mut s = Settings::default();
        s.set("language", "tr").unwrap();
        s.set("backup_keep", "20").unwrap();
        assert_eq!(s.get("language").unwrap(), "tr");
        assert_eq!(s.get("backup_keep").unwrap(), "20");
        assert!(s.set("language", "de").is_err());
        assert!(s.set("backup_keep", "0").is_err());
        assert!(s.set("backup_keep", "many").is_err());
        s.set("theme", "Light").unwrap();
        assert_eq!(s.get("theme").unwrap(), "light");
        assert!(s.set("theme", "solarized").is_err());
        assert!(s.set("nope", "1").is_err());
        assert!(s.get("nope").is_err());
    }
}
