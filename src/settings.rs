//! User settings, in `backlog/config.toml` in `$XDG_CONFIG_HOME` (default
//! `~/.config`). The file holds one `key = value` line for each setting:
//!
//! ```toml
//! details = true
//! image_width = 20
//! author = "Jane Doe"
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
    /// Free text, written as a quoted string.
    Text,
}

pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// The default value; 1 is on and 0 is off for a toggle. A text has no
    /// default value and starts empty.
    pub default: u16,
}

/// All settings, in the order of the settings popup.
pub const SETTINGS: [Setting; 8] = [
    Setting {
        key: "author",
        label: "Author",
        kind: Kind::Text,
        default: 0,
    },
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
    Setting {
        key: "rounded",
        label: "Rounded corners",
        kind: Kind::Toggle,
        default: 1,
    },
];

/// The index of each setting in `SETTINGS`.
pub const AUTHOR: usize = 0;
pub const DETAILS: usize = 1;
pub const SPLIT: usize = 2;
pub const HIDE_CLOSED: usize = 3;
pub const NEWEST_FIRST: usize = 4;
pub const IMAGES: usize = 5;
pub const IMAGE_WIDTH: usize = 6;
pub const ROUNDED: usize = 7;

type Values = [u16; SETTINGS.len()];

pub struct Settings {
    values: Values,
    author: String,
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
        let (values, author) = parse(&text);
        Settings {
            values,
            author,
            path,
        }
    }

    /// The default values, without a file.
    #[cfg(test)]
    pub fn defaults() -> Settings {
        Settings {
            values: SETTINGS.map(|s| s.default),
            author: String::new(),
            path: None,
        }
    }

    pub fn get(&self, n: usize) -> u16 {
        self.values[n]
    }

    pub fn is_on(&self, n: usize) -> bool {
        self.values[n] != 0
    }

    /// The name of the user, the author of new issues; empty if not set.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Set the name of the user and write the file if the name changes.
    pub fn set_author(&mut self, name: &str) -> io::Result<()> {
        let name = name.trim();
        if name == self.author {
            return Ok(());
        }
        self.author = name.to_string();
        self.save()
    }

    /// Change the setting `n` and write the file: toggle a toggle, and move a
    /// number by `dir` steps. A text does not change.
    pub fn change(&mut self, n: usize, dir: i32) -> io::Result<()> {
        let value = match SETTINGS[n].kind {
            Kind::Toggle => u16::from(!self.is_on(n)),
            Kind::Number { step, .. } => {
                (self.values[n] as i32 + dir * step as i32).clamp(0, u16::MAX as i32) as u16
            }
            Kind::Text => return Ok(()),
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
        fs::write(path, update(&old, &self.values, &self.author))
    }
}

fn clamp(n: usize, value: u16) -> u16 {
    match SETTINGS[n].kind {
        Kind::Toggle => value.min(1),
        Kind::Number { min, max, .. } => value.clamp(min, max),
        Kind::Text => 0,
    }
}

/// The value of the setting `n` as text: `true` / `false`, a number, or a
/// quoted string.
fn format(n: usize, values: &Values, author: &str) -> String {
    match SETTINGS[n].kind {
        Kind::Toggle => (values[n] != 0).to_string(),
        Kind::Number { .. } => values[n].to_string(),
        Kind::Text => quote(author),
    }
}

/// A TOML basic string.
fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The text of a TOML basic string, or the trimmed text if it has no quotes.
fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

fn config_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("backlog").join("config.toml"))
}

/// Split a line into its key and value, without a trailing comment. A `#`
/// in a quoted value does not start a comment.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    if key.contains('#') {
        return None;
    }
    let value = value.trim();
    let value = match value.strip_prefix('"') {
        Some(rest) => {
            let mut escaped = false;
            let end = rest.char_indices().find(|&(_, c)| {
                let end = c == '"' && !escaped;
                escaped = c == '\\' && !escaped;
                end
            });
            match end {
                Some((i, _)) => &value[..i + 2],
                None => value,
            }
        }
        None => value.split('#').next()?.trim(),
    };
    Some((key.trim(), value))
}

fn parse(text: &str) -> (Values, String) {
    let mut values = SETTINGS.map(|s| s.default);
    let mut author = String::new();
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
            (Kind::Text, v) => {
                author = unquote(v).trim().to_string();
                continue;
            }
            _ => continue,
        };
        values[n] = value;
    }
    (values, author)
}

/// Write `values` into the text of the settings file: replace the lines of
/// the known keys, keep all other lines, and add the missing keys at the end.
fn update(old: &str, values: &Values, author: &str) -> String {
    let mut written = [false; SETTINGS.len()];
    let mut out = String::new();
    for line in old.lines() {
        let known = key_value(line).and_then(|(k, _)| SETTINGS.iter().position(|s| s.key == k));
        match known {
            Some(n) if !written[n] => {
                out.push_str(&format!(
                    "{} = {}\n",
                    SETTINGS[n].key,
                    format(n, values, author)
                ));
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
            out.push_str(&format!("{} = {}\n", s.key, format(n, values, author)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_values() {
        assert_eq!(parse("").0[DETAILS], 1);
        assert_eq!(parse("details = false\n").0[DETAILS], 0);
        assert_eq!(parse("# c\ndetails=false # off\nother = 1\n").0[DETAILS], 0);
        assert_eq!(parse("details = maybe\n").0[DETAILS], 1);
        assert_eq!(parse("image_width = 30\n").0[IMAGE_WIDTH], 30);
        assert_eq!(parse("image_width = 500\n").0[IMAGE_WIDTH], 60);
        assert_eq!(parse("image_width = wide\n").0[IMAGE_WIDTH], 20);
        assert_eq!(parse("").1, "");
        assert_eq!(
            parse("author = \"Jane \\\"J#\\\" Doe\" # me\n").1,
            "Jane \"J#\" Doe"
        );
        assert_eq!(parse("author = Jane # me\n").1, "Jane");
    }

    #[test]
    fn update_keeps_other_lines() {
        let mut values = SETTINGS.map(|s| s.default);
        values[DETAILS] = 0;
        let all = "author = \"Jane \\\"J\\\" Doe\"\ndetails = false\nsplit = 45\nhide_closed = false\nnewest_first = false\nimages = true\nimage_width = 20\nrounded = true\n";
        let author = "Jane \"J\" Doe";
        assert_eq!(update("", &values, author), all);
        assert_eq!(parse(all).1, author);
        assert_eq!(
            update("# Settings\ndetails = true\nother = 1\n", &values, author),
            "# Settings\ndetails = false\nother = 1\nauthor = \"Jane \\\"J\\\" Doe\"\nsplit = 45\nhide_closed = false\nnewest_first = false\nimages = true\nimage_width = 20\nrounded = true\n"
        );
        assert_eq!(
            update(
                "author = \"x\"\ndetails = true\ndetails = true\n",
                &values,
                author
            ),
            all
        );
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
