//! The backlog model and its file format.
//!
//! A backlog is a `.backlog` directory. Each issue is one Markdown file
//! `issues/<id>.md` with a YAML front matter:
//!
//! ```markdown
//! ---
//! id: k3f9qz
//! status: open
//! created: 2026-01-31T14:05:00Z
//! closed:
//! author: Jane Doe
//! ---
//!
//! # Title of the issue
//!
//! ## Description
//! Free text.
//!
//! ## Notes
//! Free text.
//!
//! ## Outcome
//! Free text.
//! ```
//!
//! The file name gives the id. The optional file `prompt.md` holds the
//! template of the agent prompt. Images are in `images/`. Each issue has its
//! own file, so that several users can change different issues at the same
//! time and merge the changes without conflicts.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DIR_NAME: &str = ".backlog";
const ISSUES: &str = "issues";
const PROMPT_FILE: &str = "prompt.md";
/// The characters of an id: digits and lowercase letters without `i`, `l`,
/// `o` and `u`, which are easy to confuse.
const ID_CHARS: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const ID_LEN: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Open,
    Active,
    Done,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Active => "active",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> Status {
        match s.trim() {
            "active" => Status::Active,
            "done" => Status::Done,
            "cancelled" | "canceled" => Status::Cancelled,
            _ => Status::Open,
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Status::Done | Status::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub status: Status,
    /// The creation time, in UTC as `2026-01-31T14:05:00Z`.
    pub created: String,
    /// The time when the issue became done or cancelled; empty if it is open.
    pub closed: String,
    pub author: String,
    pub description: String,
    pub notes: String,
    pub outcome: String,
    /// Unknown keys of the front matter with their raw values, kept in
    /// their order.
    pub extra: Vec<(String, String)>,
}

impl Issue {
    /// A new open issue, created now.
    pub fn new(id: String, author: &str) -> Issue {
        Issue {
            id,
            title: String::new(),
            status: Status::Open,
            created: now(),
            closed: String::new(),
            author: author.to_string(),
            description: String::new(),
            notes: String::new(),
            outcome: String::new(),
            extra: Vec::new(),
        }
    }

    /// Set the status. The close time is set when the issue becomes done or
    /// cancelled, and cleared when it becomes open or active again.
    pub fn set_status(&mut self, status: Status) {
        if !status.is_closed() {
            self.closed.clear();
        } else if !self.status.is_closed() || self.closed.is_empty() {
            self.closed = now();
        }
        self.status = status;
    }

    fn render(&self) -> String {
        let mut out = String::from("---\n");
        let fields = [
            ("id", yaml_value(&self.id)),
            ("status", self.status.as_str().to_string()),
            ("created", yaml_value(&self.created)),
            ("closed", yaml_value(&self.closed)),
            ("author", yaml_value(&self.author)),
        ];
        let extra = self.extra.iter().map(|(k, v)| (k.as_str(), v.clone()));
        for (key, value) in fields.into_iter().chain(extra) {
            out.push_str(format!("{key}: {value}").trim_end());
            out.push('\n');
        }
        out.push_str("---\n\n");
        out.push_str(format!("# {}", self.title).trim_end());
        out.push('\n');
        for (name, body) in [
            ("Description", &self.description),
            ("Notes", &self.notes),
            ("Outcome", &self.outcome),
        ] {
            out.push_str(&format!("\n## {name}\n"));
            if !body.trim().is_empty() {
                out.push_str(body.trim_end());
                out.push('\n');
            }
        }
        out
    }

    /// Parse the text of the file of the issue `id`. The file name, not the
    /// front matter, gives the id.
    fn parse(id: &str, text: &str) -> Issue {
        enum Section {
            Head,
            Description,
            Notes,
            Outcome,
        }

        let mut issue = Issue::new(id.to_string(), "");
        issue.created.clear();
        let mut lines = text.lines().peekable();
        if lines.peek().is_some_and(|l| l.trim_end() == "---") {
            lines.next();
            for line in lines.by_ref() {
                if line.trim_end() == "---" {
                    break;
                }
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let raw = value.trim();
                let value = yaml_text(raw);
                match key.trim() {
                    "id" => {}
                    "status" => issue.status = Status::parse(&value),
                    "created" => issue.created = value,
                    "closed" => issue.closed = value,
                    "author" => issue.author = value,
                    key => issue.extra.push((key.to_string(), raw.to_string())),
                }
            }
        }

        let mut section = Section::Head;
        let mut has_title = false;
        for line in lines {
            // Only the known section names start a section. Other headings,
            // such as the Markdown headings in a field, belong to the
            // current section.
            if let Some(name) = line.strip_prefix("## ") {
                let next = match name.trim().to_lowercase().as_str() {
                    "description" => Some(Section::Description),
                    "notes" => Some(Section::Notes),
                    "outcome" => Some(Section::Outcome),
                    _ => None,
                };
                if let Some(next) = next {
                    section = next;
                    continue;
                }
            }
            let body = match section {
                Section::Description => &mut issue.description,
                Section::Notes => &mut issue.notes,
                Section::Outcome => &mut issue.outcome,
                Section::Head => {
                    if !has_title && (line == "#" || line.starts_with("# ")) {
                        issue.title = line[1..].trim().to_string();
                        has_title = true;
                    }
                    continue;
                }
            };
            body.push_str(line);
            body.push('\n');
        }
        for body in [&mut issue.description, &mut issue.notes, &mut issue.outcome] {
            *body = body.trim_matches('\n').to_string();
        }
        issue
    }
}

/// The names, modification times and sizes of the files of a backlog.
type Stamp = Vec<(PathBuf, SystemTime, u64)>;

pub struct Backlog {
    /// The `.backlog` directory.
    pub dir: PathBuf,
    /// The template of the agent prompt, from `prompt.md`.
    pub prompt: Option<String>,
    /// The issues, ordered by creation time.
    pub issues: Vec<Issue>,
    /// The issues as they are on disk after the last load or save. A save
    /// writes only the issues that differ from these.
    on_disk: HashMap<String, Issue>,
    prompt_on_disk: Option<String>,
    /// The files at the last load or save.
    stamp: Stamp,
}

fn stamp(dir: &Path) -> Stamp {
    let mut files: Vec<PathBuf> = fs::read_dir(dir.join(ISSUES))
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    files.push(dir.join(PROMPT_FILE));
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let meta = fs::metadata(&p).ok()?;
            Some((p, meta.modified().ok()?, meta.len()))
        })
        .collect()
}

/// Write `text` to `path` atomically through a temporary sibling file.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

impl Backlog {
    /// The directory of the issue files. Relative image paths start here.
    pub fn base_dir(&self) -> PathBuf {
        self.dir.join(ISSUES)
    }

    /// The path of the file of the issue `id`.
    pub fn issue_path(&self, id: &str) -> PathBuf {
        self.dir.join(ISSUES).join(format!("{id}.md"))
    }

    /// Search `start` and its ancestors for the backlog directory.
    pub fn find(start: &Path) -> Option<PathBuf> {
        start
            .ancestors()
            .map(|dir| dir.join(DIR_NAME))
            .find(|p| p.is_dir())
    }

    pub fn create(dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir.join(ISSUES))
    }

    pub fn load(dir: &Path) -> io::Result<Backlog> {
        let stamp = stamp(dir);
        let mut issues = Vec::new();
        let entries = match fs::read_dir(dir.join(ISSUES)) {
            Ok(entries) => entries.collect::<io::Result<Vec<_>>>()?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            issues.push(Issue::parse(id, &fs::read_to_string(&path)?));
        }
        issues.sort_by(|a, b| (&a.created, &a.id).cmp(&(&b.created, &b.id)));
        let prompt = match fs::read_to_string(dir.join(PROMPT_FILE)) {
            Ok(text) => Some(text.trim().to_string()).filter(|p| !p.is_empty()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Backlog {
            dir: dir.to_path_buf(),
            on_disk: issues.iter().map(|i| (i.id.clone(), i.clone())).collect(),
            prompt_on_disk: prompt.clone(),
            prompt,
            issues,
            stamp,
        })
    }

    /// Tell if another program changed a file since the last load or save.
    pub fn changed_on_disk(&self) -> bool {
        stamp(&self.dir) != self.stamp
    }

    /// Write the changed issues and the prompt, and remove the files of the
    /// deleted issues. The files of other issues stay as they are.
    pub fn save(&mut self) -> io::Result<()> {
        fs::create_dir_all(self.dir.join(ISSUES))?;
        for issue in &self.issues {
            if self.on_disk.get(&issue.id) != Some(issue) {
                write_atomic(&self.issue_path(&issue.id), &issue.render())?;
                self.on_disk.insert(issue.id.clone(), issue.clone());
            }
        }
        let deleted: Vec<String> = self
            .on_disk
            .keys()
            .filter(|id| !self.issues.iter().any(|i| &i.id == *id))
            .cloned()
            .collect();
        for id in deleted {
            match fs::remove_file(self.issue_path(&id)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            self.on_disk.remove(&id);
        }
        if self.prompt != self.prompt_on_disk {
            let path = self.dir.join(PROMPT_FILE);
            match &self.prompt {
                Some(p) => write_atomic(&path, &format!("{}\n", p.trim_end()))?,
                None => match fs::remove_file(&path) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                },
            }
            self.prompt_on_disk = self.prompt.clone();
        }
        self.stamp = stamp(&self.dir);
        Ok(())
    }

    /// A new random id that no issue in memory or on disk has.
    pub fn new_id(&self) -> String {
        loop {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u128(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
            );
            let mut n = hasher.finish();
            let id: String = (0..ID_LEN)
                .map(|_| {
                    let c = ID_CHARS[(n % 32) as usize] as char;
                    n /= 32;
                    c
                })
                .collect();
            if !self.issues.iter().any(|i| i.id == id) && !self.issue_path(&id).exists() {
                return id;
            }
        }
    }
}

/// `text` as a YAML scalar: plain if YAML reads it back as the same
/// string, else in double quotes.
fn yaml_value(text: &str) -> String {
    let plain = text.is_empty()
        || (!text.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c) || c.is_whitespace())
            && !text.ends_with(|c: char| c == ':' || c.is_whitespace())
            && !text.contains(": ")
            && !text.contains(" #")
            && !text.chars().any(char::is_control)
            && !matches!(
                text.to_lowercase().as_str(),
                "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
            )
            && text.parse::<f64>().is_err());
    if plain {
        return text.to_string();
    }
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of a YAML scalar in plain, single-quoted or double-quoted style.
fn yaml_text(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(c) => out.push(c),
            None => {}
        }
    }
    out
}

/// The current time in UTC as `2026-01-31T14:05:00Z`.
fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_time(secs)
}

/// Format seconds since the Unix epoch as a UTC time `2026-01-31T14:05:00Z`.
fn format_time(secs: u64) -> String {
    let (days, rest) = ((secs / 86400) as i64, secs % 86400);
    // The civil date of a day number (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "---\nid: k3f9qz\nstatus: done\ncreated: 2026-01-31T14:05:00Z\nclosed: 2026-02-01T08:00:00Z\nauthor: Jane Doe\nlabel: ui\n---\n\n# First\n\n## Description\nline 1\n\n## Details\nline 2\n\n## Notes\n\n## Outcome\n### Changes\n- fixed\n";

    #[test]
    fn round_trip() {
        let issue = Issue::parse("k3f9qz", TEXT);
        assert_eq!(issue.status, Status::Done);
        assert_eq!(issue.title, "First");
        assert_eq!(issue.author, "Jane Doe");
        assert_eq!(issue.closed, "2026-02-01T08:00:00Z");
        assert_eq!(issue.description, "line 1\n\n## Details\nline 2");
        assert_eq!(issue.notes, "");
        assert_eq!(issue.outcome, "### Changes\n- fixed");
        assert_eq!(issue.render(), TEXT);
    }

    #[test]
    fn issue_without_title() {
        let mut issue = Issue::new("abc".into(), "Jane");
        issue.description = "Do it.".into();
        let text = issue.render();
        assert!(text.contains("\n#\n\n## Description\nDo it.\n"), "{text}");
        assert_eq!(Issue::parse("abc", &text), issue);
    }

    #[test]
    fn close_time() {
        let mut issue = Issue::new("abc".into(), "");
        issue.set_status(Status::Active);
        assert_eq!(issue.closed, "");
        issue.set_status(Status::Done);
        assert_ne!(issue.closed, "");
        let closed = issue.closed.clone();
        issue.closed = "2000-01-01T00:00:00Z".into();
        issue.set_status(Status::Cancelled);
        assert_eq!(issue.closed, "2000-01-01T00:00:00Z");
        issue.set_status(Status::Open);
        assert_eq!(issue.closed, "");
        assert_eq!(closed.len(), 20);
    }

    #[test]
    fn yaml_quoting() {
        for (text, yaml) in [
            ("Jane Doe", "Jane Doe"),
            ("", ""),
            ("Doe: Jane", "\"Doe: Jane\""),
            ("#1 Dev", "\"#1 Dev\""),
            ("O'Brien", "O'Brien"),
            ("\"Q\" \\ x", "\"\\\"Q\\\" \\\\ x\""),
            ("true", "\"true\""),
            ("123", "\"123\""),
            ("- x", "\"- x\""),
        ] {
            assert_eq!(yaml_value(text), yaml, "{text}");
            assert_eq!(yaml_text(yaml), text, "{yaml}");
        }
        assert_eq!(yaml_text("'it''s'"), "it's");
        let mut issue = Issue::new("abc".into(), "Doe: Jane");
        issue.extra.push(("labels".into(), "[a, b]".into()));
        let text = issue.render();
        assert!(
            text.contains("\nauthor: \"Doe: Jane\"\nlabels: [a, b]\n"),
            "{text}"
        );
        assert_eq!(Issue::parse("abc", &text), issue);
    }

    #[test]
    fn time_format() {
        assert_eq!(format_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_time(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_time(1_769_868_300), "2026-01-31T14:05:00Z");
    }

    #[test]
    fn save_writes_only_changes() {
        let dir = std::env::temp_dir().join(format!("backlog-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Backlog::create(&dir).unwrap();
        let mut b = Backlog::load(&dir).unwrap();
        for n in 0..2 {
            let mut issue = Issue::new(b.new_id(), "Jane");
            issue.title = format!("Issue {n}");
            b.issues.push(issue);
        }
        b.prompt = Some("Do {id}.".into());
        b.save().unwrap();
        let (a, c) = (b.issues[0].id.clone(), b.issues[1].id.clone());

        // Another program changes the second issue; a change of the first
        // issue keeps that change.
        fs::write(b.issue_path(&c), "---\nstatus: active\n---\n\n# Other\n").unwrap();
        b.issues[0].title = "Changed".into();
        b.save().unwrap();
        let loaded = Backlog::load(&dir).unwrap();
        assert_eq!(loaded.issues.len(), 2);
        assert_eq!(loaded.prompt.as_deref(), Some("Do {id}."));
        let get = |id: &str| loaded.issues.iter().find(|i| i.id == id).unwrap();
        assert_eq!(get(&a).title, "Changed");
        assert_eq!(get(&c).title, "Other");

        b.issues.retain(|i| i.id != a);
        b.prompt = None;
        b.save().unwrap();
        assert!(!b.issue_path(&a).exists());
        assert!(!dir.join(PROMPT_FILE).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
