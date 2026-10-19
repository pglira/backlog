//! The backlog model and its Markdown file format.
//!
//! File layout:
//!
//! ```markdown
//! # Backlog
//!
//! ## #1 Title of the issue
//! status: open
//!
//! ### Description
//! Free text.
//!
//! ### Done when
//! Free text.
//!
//! ### Notes
//! Free text.
//! ```
//!
//! Text before the first issue is kept verbatim. New issues go to the end.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const FILE_NAME: &str = ".backlog.md";
const HEADER: &str = "# Backlog\n";

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

#[derive(Clone, Debug)]
pub struct Issue {
    pub id: u32,
    pub title: String,
    pub status: Status,
    pub description: String,
    pub done_when: String,
    pub notes: String,
}

pub struct Backlog {
    pub path: PathBuf,
    preamble: String,
    pub issues: Vec<Issue>,
    /// Modification time and size of the file at the last load or save.
    stamp: Option<(SystemTime, u64)>,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl Backlog {
    /// Search `start` and its ancestors for the backlog file.
    pub fn find(start: &Path) -> Option<PathBuf> {
        start
            .ancestors()
            .map(|dir| dir.join(FILE_NAME))
            .find(|p| p.is_file())
    }

    pub fn create(path: &Path) -> io::Result<()> {
        fs::write(path, HEADER)
    }

    pub fn load(path: &Path) -> io::Result<Backlog> {
        let stamp = stamp(path);
        let text = fs::read_to_string(path)?;
        let (preamble, issues) = parse(&text);
        Ok(Backlog {
            path: path.to_path_buf(),
            preamble,
            issues,
            stamp,
        })
    }

    /// Tell if another program changed the file since the last load or save.
    /// A missing file counts as unchanged; the next save creates it again.
    pub fn changed_on_disk(&self) -> bool {
        stamp(&self.path).is_some_and(|s| Some(s) != self.stamp)
    }

    /// Write the file atomically through a temporary sibling file.
    pub fn save(&mut self) -> io::Result<()> {
        let tmp = self.path.with_extension("md.tmp");
        fs::write(&tmp, self.render())?;
        fs::rename(&tmp, &self.path)?;
        self.stamp = stamp(&self.path);
        Ok(())
    }

    pub fn next_id(&self) -> u32 {
        self.issues.iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    fn render(&self) -> String {
        let mut out = self.preamble.trim_end().to_string();
        out.push('\n');
        for i in &self.issues {
            out.push_str(&format!(
                "\n## #{} {}\nstatus: {}\n",
                i.id,
                i.title,
                i.status.as_str()
            ));
            for (name, body) in [
                ("Description", &i.description),
                ("Done when", &i.done_when),
                ("Notes", &i.notes),
            ] {
                if !body.trim().is_empty() {
                    out.push_str(&format!("\n### {}\n{}\n", name, body.trim_end()));
                }
            }
        }
        out
    }
}

/// Parse an issue heading of the form `## #<id> <title>`.
fn parse_heading(line: &str) -> Option<(u32, String)> {
    let rest = line.strip_prefix("## #")?;
    let (id, title) = rest.split_once(' ').unwrap_or((rest, ""));
    Some((id.parse().ok()?, title.trim().to_string()))
}

fn parse(text: &str) -> (String, Vec<Issue>) {
    enum Section {
        None,
        Description,
        DoneWhen,
        Notes,
    }

    let mut preamble = String::new();
    let mut issues: Vec<Issue> = Vec::new();
    let mut section = Section::None;

    for line in text.lines() {
        if let Some((id, title)) = parse_heading(line) {
            issues.push(Issue {
                id,
                title,
                status: Status::Open,
                description: String::new(),
                done_when: String::new(),
                notes: String::new(),
            });
            section = Section::None;
            continue;
        }
        let Some(issue) = issues.last_mut() else {
            preamble.push_str(line);
            preamble.push('\n');
            continue;
        };
        if let Some(name) = line.strip_prefix("### ") {
            section = match name.trim().to_lowercase().as_str() {
                "description" => Section::Description,
                "done when" => Section::DoneWhen,
                "notes" => Section::Notes,
                _ => Section::None,
            };
            continue;
        }
        let body = match section {
            Section::Description => &mut issue.description,
            Section::DoneWhen => &mut issue.done_when,
            Section::Notes => &mut issue.notes,
            Section::None => {
                if let Some(s) = line.strip_prefix("status:") {
                    issue.status = Status::parse(s);
                }
                continue;
            }
        };
        body.push_str(line);
        body.push('\n');
    }

    for issue in &mut issues {
        for body in [
            &mut issue.description,
            &mut issue.done_when,
            &mut issue.notes,
        ] {
            *body = body.trim_matches('\n').to_string();
        }
    }
    if preamble.trim().is_empty() {
        preamble = HEADER.to_string();
    }
    (preamble, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = "# Backlog\n\nNotes.\n\n## #1 First\nstatus: done\n\n### Description\nline 1\n\nline 2\n\n### Done when\nit works\n\n### Notes\nsee #4\n\n## #4 Second\nstatus: open\n";
        let (preamble, issues) = parse(text);
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].status, Status::Done);
        assert_eq!(issues[0].description, "line 1\n\nline 2");
        assert_eq!(issues[0].done_when, "it works");
        assert_eq!(issues[0].notes, "see #4");
        assert_eq!(issues[1].id, 4);
        let b = Backlog {
            path: PathBuf::new(),
            preamble,
            issues,
            stamp: None,
        };
        assert_eq!(b.render(), text);
        assert_eq!(b.next_id(), 5);
    }
}
