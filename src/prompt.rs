//! The template of the prompt for a coding agent.
//!
//! The `## Agent prompt` section of the backlog file holds the template.
//! Without that section, the built-in default applies. `{id}`, `{title}`
//! and `{path}` in the template are replaced by the values of the issue.

pub const DEFAULT: &str = "Work on issue #{id} \"{title}\" from the backlog file {path}. \
Read the issue there first: its description says what to do, \
and its \"Done when\" section (if present) says when the work is complete. \
Before you start, set the status line of the issue to \"status: active\". \
Never set the status to \"done\" without an explicit request from me. \
When I ask you to set it to \"done\", also write a short summary of the work \
in the \"### Outcome\" section of the issue; do not write the Outcome before that.";

/// Replace the placeholders in `template` with the values of an issue.
pub fn fill(template: &str, id: u32, title: &str, path: &str) -> String {
    template
        .replace("{id}", &id.to_string())
        .replace("{title}", title)
        .replace("{path}", path)
}
