use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModifierKey {
    Shift,
    Control,
}

impl ModifierKey {
    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("shift") {
            Some(Self::Shift)
        } else if value.eq_ignore_ascii_case("control") || value.eq_ignore_ascii_case("ctrl") {
            Some(Self::Control)
        } else {
            None
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Shift => Self::Control,
            Self::Control => Self::Shift,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Control => "Ctrl",
        }
    }

    #[cfg(target_os = "windows")]
    pub const fn is_pressed(self, shift_pressed: bool, control_pressed: bool) -> bool {
        match self {
            Self::Shift => shift_pressed,
            Self::Control => control_pressed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifierSettings {
    pub convert_modifier: ModifierKey,
    pub actions_modifier: ModifierKey,
}

impl Default for ModifierSettings {
    fn default() -> Self {
        Self {
            convert_modifier: ModifierKey::Shift,
            actions_modifier: ModifierKey::Control,
        }
    }
}

impl ModifierSettings {
    pub fn from_convert_modifier(value: &str) -> Result<Self, String> {
        let convert_modifier = ModifierKey::parse(value)
            .ok_or_else(|| "Dönüştürme tuşu Shift veya Control olmalıdır".to_owned())?;
        Ok(Self {
            convert_modifier,
            actions_modifier: convert_modifier.other(),
        })
    }
}

pub fn load() -> ModifierSettings {
    let Ok(contents) = fs::read_to_string(settings_path()) else {
        return ModifierSettings::default();
    };
    let Ok(settings) = serde_json::from_str::<ModifierSettings>(&contents) else {
        return ModifierSettings::default();
    };
    if settings.convert_modifier == settings.actions_modifier {
        return ModifierSettings::default();
    }
    settings
}

pub fn save(settings: ModifierSettings) -> Result<(), String> {
    let path = settings_path();
    let parent = path
        .parent()
        .ok_or_else(|| "Ayar dizini belirlenemedi".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("Ayar dizini oluşturulamadı: {error}"))?;
    let contents = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("Ayarlar hazırlanamadı: {error}"))?;
    fs::write(path, format!("{contents}\n"))
        .map_err(|error| format!("Ayarlar kaydedilemedi: {error}"))
}

pub fn settings_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(root) = std::env::var_os("APPDATA") {
        return PathBuf::from(root).join("Grapefruit").join("settings.json");
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("grapefruit").join("settings.json");
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("grapefruit")
        .join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_shift_for_convert_and_control_for_actions() {
        assert_eq!(
            ModifierSettings::default(),
            ModifierSettings {
                convert_modifier: ModifierKey::Shift,
                actions_modifier: ModifierKey::Control,
            }
        );
    }

    #[test]
    fn selecting_control_swaps_both_actions() {
        let settings = ModifierSettings::from_convert_modifier("control").unwrap();
        assert_eq!(settings.convert_modifier, ModifierKey::Control);
        assert_eq!(settings.actions_modifier, ModifierKey::Shift);
    }
}
