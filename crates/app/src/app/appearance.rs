use proto::{ErrorCode, Response};
use serde::Serialize;
use serde_json::json;

use super::{App, ok};
use crate::i18n::{Arg, tr, trf};
use crate::store;
use crate::style::{self, Accent, GENERAL, Group, Key, StyleError, UserStyles};

#[derive(Serialize)]
pub struct StyleEntry {
    name: String,
    built_in: bool,
    active: bool,
    accent: String,
}

fn error(error: &StyleError) -> Response {
    let code = match error {
        StyleError::Missing(_) => ErrorCode::NotFound,
        _ => ErrorCode::Invalid,
    };
    return Response::err(code, error.to_string());
}

pub fn reply<T: Serialize>(result: Result<T, StyleError>) -> Response {
    return match result {
        Ok(value) => ok(&value),
        Err(failure) => error(&failure),
    };
}

impl App {
    pub fn styles(&self) -> &UserStyles {
        return &self.styles;
    }

    fn save_styles(&self) {
        if let Err(error) = store::save(&self.paths.styles_file(), &self.styles) {
            tracing::warn!(%error, "cannot save styles");
        }
    }

    fn activate(&mut self, name: &str) {
        if self.settings.appearance.style != name {
            self.settings.appearance.style = name.to_string();
            self.save_settings();
        }
    }

    pub(super) fn load_styles(&mut self, legacy: bool, migrated: bool) {
        if legacy && migrated {
            self.save_styles();
            self.persist_settings();
        }
        if self.library.migrate_colors() {
            self.persist_library();
        }
    }

    pub fn style_list(&self) -> Vec<StyleEntry> {
        let active = self.styles.active(&self.settings.appearance.style).name;
        return self
            .styles
            .all()
            .into_iter()
            .map(|entry| StyleEntry {
                built_in: style::built_in(&entry.name),
                active: entry.name == active,
                accent: match entry.accent {
                    Accent::System => "system".to_string(),
                    Accent::Fixed(pair) => pair.light.to_hex(),
                },
                name: entry.name,
            })
            .collect();
    }

    pub fn style_apply(&mut self, name: &str) -> Result<String, StyleError> {
        let found = self.styles.style(name).ok_or_else(|| StyleError::Missing(name.to_string()))?;
        self.activate(&found.name);
        crate::ui::restyled();
        return Ok(found.name);
    }

    pub fn style_export(&self, name: Option<&str>) -> Result<(String, String), StyleError> {
        let name = name.unwrap_or(&self.settings.appearance.style);
        let found = match self.styles.style(name) {
            Some(found) => found,
            None if name == self.settings.appearance.style => self.styles.active(name),
            None => return Err(StyleError::Missing(name.to_string())),
        };
        let text = style::render(&found);
        return Ok((found.name, text));
    }

    pub fn style_import(&mut self, text: &str) -> Result<String, StyleError> {
        let name = self.styles.import(text)?;
        self.save_styles();
        crate::ui::restyled();
        return Ok(name);
    }

    pub fn style_delete(&mut self, name: &str) -> Result<String, StyleError> {
        let removed = self.styles.remove(name)?;
        self.save_styles();
        if removed.name.eq_ignore_ascii_case(&self.settings.appearance.style) {
            self.activate(style::SYSTEM);
        }
        crate::ui::restyled();
        return Ok(removed.name);
    }

    pub fn style_rename(&mut self, name: &str, to: &str) -> Result<String, StyleError> {
        let renamed = self.styles.rename(name, to)?;
        self.save_styles();
        if name.trim().eq_ignore_ascii_case(&self.settings.appearance.style) {
            self.activate(&renamed);
        }
        crate::ui::restyled();
        return Ok(renamed);
    }

    fn edit(&mut self, change: impl FnOnce(&mut style::Style) -> Result<(), StyleError>) -> Result<String, StyleError> {
        let active = self.styles.active(&self.settings.appearance.style);
        if style::built_in(&active.name) {
            let mut fork = active.clone();
            change(&mut fork)?;
            fork.name = trf("%@ (edited)", &[Arg::Text(tr(&active.name))]);
            let name = self.styles.add(fork);
            self.save_styles();
            self.activate(&name);
            crate::ui::restyled();
            return Ok(name);
        }
        let Some(style) = self.styles.find_mut(&active.name) else {
            return Err(StyleError::Missing(active.name));
        };
        change(style)?;
        let name = style.name.clone();
        self.save_styles();
        self.activate(&name);
        crate::ui::restyled();
        return Ok(name);
    }

    pub fn style_set(&mut self, field: &str, light: &str, dark: Option<&str>) -> Result<String, StyleError> {
        return self.edit(|style| style.set(field, light, dark));
    }

    pub fn style_reset(&mut self, field: &str) -> Result<String, StyleError> {
        return self.edit(|style| style.reset(field));
    }

    pub fn style_keys(&self) -> serde_json::Value {
        let general: Vec<serde_json::Value> =
            GENERAL.iter().map(|(id, title)| json!({ "id": id, "title": title })).collect();
        let keys: Vec<serde_json::Value> = Group::ALL
            .iter()
            .flat_map(|group| {
                return group.keys().map(move |key: Key| {
                    return json!({ "id": key.id(), "group": group.title(), "title": key.title() });
                });
            })
            .collect();
        return json!({ "general": general, "keys": keys });
    }
}
