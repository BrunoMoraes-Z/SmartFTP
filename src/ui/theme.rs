//! Themes are tiny TOML files with 15 colors. Built-ins are embedded below;
//! user themes are loaded from `<config>/SmartFTP/themes/*.toml`.

use std::collections::BTreeMap;

use gpui::{App, Global, Hsla, WindowAppearance, rgb, rgba};
use serde::Deserialize;

use crate::core::config::{Config, ThemeMode, themes_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    pub builtin: bool,
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface2: Hsla,
    pub hover: Hsla,
    pub border: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    pub accent_ink: Hsla,
    pub accent_soft: Hsla,
    pub signal: Hsla,
    pub ok: Hsla,
    pub warn: Hsla,
    pub err: Hsla,
    pub folder: Hsla,
    pub source: String,
}

#[derive(Deserialize)]
struct ThemeFile {
    name: String,
    appearance: Appearance,
    colors: BTreeMap<String, String>,
}

pub fn parse_color(s: &str) -> Option<Hsla> {
    let hex = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(rgb(v).into()),
        8 => Some(rgba(v).into()),
        3 => {
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some(rgb((r * 17) << 16 | (g * 17) << 8 | (b * 17)).into())
        }
        _ => None,
    }
}

impl Theme {
    pub fn from_toml(id: &str, text: &str, builtin: bool) -> anyhow::Result<Theme> {
        let file: ThemeFile = toml::from_str(text)?;
        let c = |key: &str, fallback: Option<Hsla>| -> anyhow::Result<Hsla> {
            match file.colors.get(key) {
                Some(v) => parse_color(v).ok_or_else(|| anyhow::anyhow!("invalid color for {key}: {v}")),
                None => fallback.ok_or_else(|| anyhow::anyhow!("missing color '{key}'")),
            }
        };
        let ink = c("ink", None)?;
        let accent = c("accent", None)?;
        let warn = c("warn", None)?;
        Ok(Theme {
            id: id.to_string(),
            name: file.name,
            appearance: file.appearance,
            builtin,
            bg: c("bg", None)?,
            surface: c("surface", None)?,
            surface2: c("surface2", None)?,
            hover: c("hover", None)?,
            border: c("border", None)?,
            ink,
            muted: c("muted", None)?,
            faint: c("faint", None)?,
            accent,
            accent_ink: c("accent_ink", None)?,
            accent_soft: c("accent_soft", Some(accent.opacity(0.12)))?,
            signal: c("signal", Some(warn))?,
            ok: c("ok", None)?,
            warn,
            err: c("err", None)?,
            folder: c("folder", Some(rgb(0xD9A441).into()))?,
            source: text.to_string(),
        })
    }
}

pub const BUILTIN: &[(&str, &str)] = &[
    ("graphite-dark", include_str!("../../themes/graphite-dark.toml")),
    ("graphite-light", include_str!("../../themes/graphite-light.toml")),
    ("github-dark", include_str!("../../themes/github-dark.toml")),
    ("github-light", include_str!("../../themes/github-light.toml")),
    ("one-dark-pro", include_str!("../../themes/one-dark-pro.toml")),
    ("dracula", include_str!("../../themes/dracula.toml")),
];

pub struct Themes {
    pub all: Vec<Theme>,
    pub active: Theme,
    pub errors: Vec<String>,
    pub os_appearance: Appearance,
}

impl Global for Themes {}

impl Themes {
    pub fn load(cfg: &Config, os: Appearance) -> Themes {
        let mut all: Vec<Theme> =
            BUILTIN.iter().map(|(id, src)| Theme::from_toml(id, src, true).expect("built-in theme must parse")).collect();
        let mut errors = vec![];
        if let Ok(dir) = std::fs::read_dir(themes_dir()) {
            let mut files: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
            files.sort();
            for path in files {
                let id = path.file_stem().unwrap().to_string_lossy().to_string();
                match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| Theme::from_toml(&id, &t, false)) {
                    Ok(t) => {
                        // a user file with a built-in id overrides it
                        all.retain(|x| x.id != t.id);
                        all.push(t);
                    }
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        let mut themes = Themes { active: all[0].clone(), all, errors, os_appearance: os };
        themes.select(cfg);
        themes
    }

    pub fn wanted_appearance(&self, cfg: &Config) -> Appearance {
        match cfg.theme_mode {
            ThemeMode::Light => Appearance::Light,
            ThemeMode::Dark => Appearance::Dark,
            ThemeMode::System => self.os_appearance,
        }
    }

    pub fn select(&mut self, cfg: &Config) {
        let (id, fallback) = match self.wanted_appearance(cfg) {
            Appearance::Light => (&cfg.light_theme, "graphite-light"),
            Appearance::Dark => (&cfg.dark_theme, "graphite-dark"),
        };
        self.active = self
            .all
            .iter()
            .find(|t| &t.id == id)
            .or_else(|| self.all.iter().find(|t| t.id == fallback))
            .cloned()
            .unwrap_or_else(|| self.all[0].clone());
    }
}

pub fn appearance_of(a: WindowAppearance) -> Appearance {
    match a {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        _ => Appearance::Dark,
    }
}

pub fn theme(cx: &App) -> &Theme {
    &cx.global::<Themes>().active
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_parse() {
        for (id, src) in BUILTIN {
            Theme::from_toml(id, src, true).unwrap();
        }
    }
}
