//! The template of the prompt for a coding agent.
//!
//! The file `prompt.md` in the backlog directory holds the template. Without
//! that file, the built-in default applies. `{id}`, `{title}` and `{path}`
//! in the template are replaced by the id, the title and the file path of the
//! issue.

pub const DEFAULT: &str = "Work on issue {id} \"{title}\". \
The file {path} holds the issue. Its description says what to do. \
Of the backlog, edit only this file. \
Before you start, set \"status: active\" in its front matter. \
If the issue has no title, also give it a short title (20 characters at most) \
from its description, in the heading line \"# <title>\". \
Never set the status to \"done\" without an explicit request from me. \
When I ask you to set it to \"done\", also set \"closed:\" in the front matter \
to the output of `date -u +%Y-%m-%dT%H:%M:%SZ`, and write a short summary \
of the work in the \"## Outcome\" section; do not write the Outcome before that. \
Write the Outcome as well-structured Markdown in ASD-STE100 Simplified Technical English: \
short bullet lists with **bold** labels, `code` for file names, commands and identifiers, \
and no long prose paragraphs.";

/// Replace the placeholders in `template` with the values of an issue. An
/// empty title becomes "(no title)".
pub fn fill(template: &str, id: &str, title: &str, path: &str) -> String {
    let title = if title.is_empty() {
        "(no title)"
    } else {
        title
    };
    template
        .replace("{id}", id)
        .replace("{title}", title)
        .replace("{path}", path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_names_the_file() {
        let text = fill(DEFAULT, "k3f9qz", "", "/tmp/.backlog/issues/k3f9qz.md");
        assert!(text.starts_with(
            "Work on issue k3f9qz \"(no title)\". The file /tmp/.backlog/issues/k3f9qz.md holds"
        ));
    }
}
