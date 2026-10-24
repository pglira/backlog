//! User settings, in `backlog/config.toml` in `$XDG_CONFIG_HOME` (default
//! `~/.config`). The file holds one `key = value` line for each setting:
//!
//! ```toml
//! details = true
//! image_width = 20
//! ```
//!
//! Comments, empty lines and unknown keys stay in the file when it is saved.

use std::path::PathBuf;
use std::{env, fs, io};

/// The type of the value of a setting.
pub enum Kind {
    /// On or off, written as `true` or `false`.
    Toggle,
    /// A number from `min` to `max`, changed in steps of `step`.
    Number {
        min: u16,
        max: u16,
        step: u16,
        unit: &'static str,
    },
}

pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// The default value; 1 is on and 0 is off for a toggle.
    pub default: u16,
}

/// All settings, in the order of the settings popup.
pub const SETTINGS: [Setting; 6] = [
    Setting {
        key: "details",
        label: "Show the details at start",
        kind: Kind::Toggle,
        default: 1,
    },
    Setting {
        key: "split",
        label: "Size of the list",
        kind: Kind::Number {
            min: 10,
            max: 90,
            step: 5,
            unit: "%",
        },
        default: 45,
    },
    Setting {
        key: "hide_closed",
        label: "Hide done and cancelled issues",
        kind: Kind::Toggle,
        default: 0,
    },
    Setting {
        key: "newest_first",
        label: "Show the newest issues first",
        kind: Kind::Toggle,
        default: 0,
    },
    Setting {
        key: "images",
        label: "Show image previews",
        kind: Kind::Toggle,
        default: 1,
    },
    Setting {
        key: "image_width",
        label: "Width of the image previews",
        kind: Kind::Number {
            min: 10,
            max: 60,
            step: 5,
            unit: " cells",
        },
        default: 20,
    },
];

/// The index of each setting in `SETTINGS`.
pub const DETAILS: usize = 0;
pub const SPLIT: usize = 1;
pub const HIDE_CLOSED: usize = 2;
pub const NEWEST_FIRST: usize = 3;
pub const IMAGES: usize = 4;
pub const IMAGE_WIDTH: usize = 5;

type Values = [u16; SETTINGS.len()];

pub struct Settings {
    values: Values,
    /// The path of the file; `None` if no configuration directory is known.
    path: Option<PathBuf>,
}

impl Settings {
    /// Read the settings file. A missing or unreadable file gives the defaults.
    pub fn load() -> Settings {
        let path = config_path();
        let text = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        Settings {
            values: parse(&text),
            path,
        }
    }

    /// The default values, without a file.
    #[cfg(test)]
    pub fn defaults() -> Settings {
        Settings {
            values: SETTINGS.map(|s| s.default),
            path: None,
        }
    }

    pub fn get(&self, n: usize) -> u16 {
        self.values[n]
    }

    pub fn is_on(&self, n: usize) -> bool {
        self.values[n] != 0
    }

    /// Change the setting `n` and write the file: toggle a toggle, and move a
    /// number by `dir` steps.
    pub fn change(&mut self, n: usize, dir: i32) -> io::Result<()> {
        let value = match SETTINGS[n].kind {
            Kind::Toggle => u16::from(!self.is_on(n)),
            Kind::Number { step, .. } => {
                (self.values[n] as i32 + dir * step as i32).clamp(0, u16::MAX as i32) as u16
            }
        };
        self.set(n, value)
    }

    /// Set the setting `n` to `value`, clamped to its range, and write the
    /// file if the value changes.
    pub fn set(&mut self, n: usize, value: u16) -> io::Result<()> {
        let value = clamp(n, value);
        if value == self.values[n] {
            return Ok(());
        }
        self.values[n] = value;
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other("no configuration directory"));
        };
        let old = fs::read_to_string(path).unwrap_or_default();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, update(&old, &self.values))
    }
}

fn clamp(n: usize, value: u16) -> u16 {
    match SETTINGS[n].kind {
        Kind::Toggle => value.min(1),
        Kind::Number { min, max, .. } => value.clamp(min, max),
    }
}

/// The value of the setting `n` as text: `true` / `false` or a number.
fn format(n: usize, value: u16) -> String {
    match SETTINGS[n].kind {
        Kind::Toggle => (value != 0).to_string(),
        Kind::Number { .. } => value.to_string(),
    }
}

fn config_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("backlog").join("config.toml"))
}

/// Split a line into its key and value, without a trailing comment.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let line = line.split('#').next()?;
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

fn parse(text: &str) -> Values {
    let mut values = SETTINGS.map(|s| s.default);
    for (key, value) in text.lines().filter_map(key_value) {
        let Some(n) = SETTINGS.iter().position(|s| s.key == key) else {
            continue;
        };
        let value = match (&SETTINGS[n].kind, value) {
            (Kind::Toggle, "true") => 1,
            (Kind::Toggle, "false") => 0,
            (Kind::Number { .. }, v) => match v.parse() {
                Ok(v) => clamp(n, v),
                Err(_) => continue,
            },
            _ => continue,
        };
        values[n] = value;
    }
    values
}

/// Write `values` into the text of the settings file: replace the lines of
/// the known keys, keep all other lines, and add the missing keys at the end.
fn update(old: &str, values: &Values) -> String {
    let mut written = [false; SETTINGS.len()];
    let mut out = String::new();
    for line in old.lines() {
        let known = key_value(line).and_then(|(k, _)| SETTINGS.iter().position(|s| s.key == k));
        match known {
            Some(n) if !written[n] => {
                out.push_str(&format!("{} = {}\n", SETTINGS[n].key, format(n, values[n])));
                written[n] = true;
            }
            Some(_) => {}
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    for (n, s) in SETTINGS.iter().enumerate() {
        if !written[n] {
            out.push_str(&format!("{} = {}\n", s.key, format(n, values[n])));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_values() {
        assert_eq!(parse("")[DETAILS], 1);
        assert_eq!(parse("details = false\n")[DETAILS], 0);
        assert_eq!(parse("# c\ndetails=false # off\nother = 1\n")[DETAILS], 0);
        assert_eq!(parse("details = maybe\n")[DETAILS], 1);
        assert_eq!(parse("image_width = 30\n")[IMAGE_WIDTH], 30);
        assert_eq!(parse("image_width = 500\n")[IMAGE_WIDTH], 60);
        assert_eq!(parse("image_width = wide\n")[IMAGE_WIDTH], 20);
    }

    #[test]
    fn update_keeps_other_lines() {
        let mut values = SETTINGS.map(|s| s.default);
        values[DETAILS] = 0;
        let all = "details = false\nsplit = 45\nhide_closed = false\nnewest_first = false\nimages = true\nimage_width = 20\n";
        assert_eq!(update("", &values), all);
        assert_eq!(
            update("# Settings\ndetails = true\nother = 1\n", &values),
            format!("# Settings\ndetails = false\nother = 1\n{}", &all[16..])
        );
        assert_eq!(update("details = true\ndetails = true\n", &values), all);
    }

    #[test]
    fn change_stays_in_range() {
        let mut s = Settings::defaults();
        assert!(s.change(IMAGE_WIDTH, 1).is_err());
        assert_eq!(s.get(IMAGE_WIDTH), 25);
        for _ in 0..20 {
            let _ = s.change(IMAGE_WIDTH, -1);
        }
        assert_eq!(s.get(IMAGE_WIDTH), 10);
        let _ = s.change(HIDE_CLOSED, 1);
        assert!(s.is_on(HIDE_CLOSED));
    }
}
