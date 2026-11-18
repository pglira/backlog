# backlog

A terminal UI for the issues in a `.backlog` directory. Each issue is one
Markdown file, so several users can work in the same repository and merge
their changes with git.

Static Linux binaries for x86_64 and aarch64 are on the
[releases page](https://github.com/pglira/backlog/releases). To build from source:

```sh
cargo install --path .
backlog            # nearest .backlog directory in this directory or a parent
backlog DIR        # DIR/.backlog, or DIR if it is a backlog directory
```

If the directory does not exist, `backlog` asks to create it.
When another program changes a file, `backlog` reads the backlog again. Drag the
border between the list and the details with the mouse to resize them.

The backlog directory has this layout:

```
.backlog/
├── issues/
│   ├── k3f9qz.md   # one file for each issue
│   └── 7hq2mx.md
├── images/         # pasted images
└── prompt.md       # optional template of the agent prompt
```

Each issue has a unique random id of 6 characters, which is also its file
name. An issue has a title, a description, notes, an outcome (a short
summary of the work), a status (`open`, `active`, `done` or `cancelled`),
a creation time, a close time and an author. The list shows the issues in
the order of their creation time. `backlog` writes only the files of the
changed issues, so two users who change different issues get no merge
conflict. The fields show Markdown (headings, lists, task lists,
quotes, tables, code, **bold**, *italic*, links, images) rendered; a field in edit shows
the raw text:

```markdown
---
id: k3f9qz
status: done
created: 2026-01-31T14:05:00Z
closed: 2026-02-02T09:30:00Z
author: Jane Doe
---

# Fix login timeout

## Description
The session expires after 5 minutes. A user must stay logged in for 8 hours.

## Notes
The timeout is set in `config/session.toml`.

## Outcome
The timeout is now 8 hours and comes from the configuration file.
```

The times are in UTC. `backlog` sets `closed` when an issue becomes done
or cancelled, and clears it when the issue becomes open or active again.
`author` is the name in the settings at the time of creation. Without a
name in the settings, `a` first asks for it and proposes the git
`user.name`. Values that YAML would misread, such as `Doe: Jane`, are
written in double quotes.

## Images

In a field other than the title, `ctrl-v` pastes an image from the
clipboard (with `wl-paste` or `xclip`). A pasted path of an image file, for
example from a terminal that gives a dropped file or a clipboard image as a
path, also inserts the image. `backlog` copies the image to
`.backlog/images/` and inserts a link relative to the issue file, such as
`![Image #1](../images/3f2a9c0d1b7e4a65.png)`. The file name is a
hash of the content.

The rendered fields show each image below its line, as a small preview
20 cells wide (see the settings). An agent reads the image files.
`backlog` uses the kitty, iTerm2 or sixel graphics protocol when the terminal supports one,
and Unicode half blocks if not. Remove the link to remove an image from a
field; `backlog` does not delete the image files.

## Keys

Press `?` for all keys.

| List              |                                    |
|-------------------|------------------------------------|
| `j` `k` `gg` `G`  | move                               |
| `a` `o`           | add                                |
| `enter`           | edit                               |
| `x` `s` `c`       | toggle done / active / cancelled   |
| `dd`              | delete                             |
| `y`               | copy a prompt for an agent         |
| `P`               | edit the agent prompt              |
| `E`               | edit the agent prompt in `$EDITOR` |
| `R`               | restore the default agent prompt   |
| `p`               | show / hide the details            |
| `S`               | settings                           |
| `/`               | search (also by id)                |
| `q`               | quit                               |

`S` opens the settings. `space` toggles the selected setting or edits a
text, and `h` / `l` change a number. `backlog` saves each change at once in
`backlog/config.toml` in `$XDG_CONFIG_HOME` (default `~/.config`):

```toml
author = "Jane Doe"  # your name, the author of new issues
details = true       # show the details at start
split = 45           # size of the list, in percent (10 to 90)
hide_closed = false  # hide done and cancelled issues
newest_first = false # show the newest issues first
images = true        # show image previews
image_width = 20     # width of the image previews, in cells (10 to 60)
rounded = true       # rounded corners of the borders
```

A drag of the border between the list and the details also saves `split`.

`y` copies a prompt such as `Work on issue k3f9qz "...". The file
/path/.backlog/issues/k3f9qz.md holds the issue. ...` to the clipboard. The prompt also asks the agent to
set the issue to `active` when it starts, and never to set it to `done`
without your explicit request. Only when you ask for `done` does the agent
set the close time (from `date -u`) and write a short summary of its work in the Outcome section, as structured
Markdown in ASD-STE100 Simplified Technical English. The prompt is
copied with the OSC 52 escape sequence and with `wl-copy`, `xclip` or
`xsel` when available.

An issue can have no title, but then it needs a description. The list
shows it as "(no title)" with the start of the description, and the prompt
asks the agent to give it a short title (20 characters at most).

`P` edits the prompt template in the same editor as the fields; `esc`
saves it. `E` edits the template in `$VISUAL` or `$EDITOR`
and saves it when the editor exits. `{id}`, `{title}` and `{path}` in the template are
replaced by the id, the title and the file path of the issue. An edited
template is stored in `.backlog/prompt.md`; an empty template, or one equal
to the built-in default, removes the file. `R` restores the built-in default
after a confirmation and removes the file.

In the form, `i` edits a field inline and `e` edits it in `$VISUAL` or
`$EDITOR`. `w` saves. `Y` saves and copies the agent prompt for the
issue, as `y` does in the list. Inline editing takes readline keys
(`ctrl-a`, `ctrl-e`, `ctrl-w`, `ctrl-k`, `alt-b`, ...), and `esc` stops
editing the field. `ctrl-x ctrl-e` continues in `$EDITOR`, for example in Vim.

The form shows the outcome in its right half. A field with more text than
space shows a scroll bar; the mouse wheel or `ctrl-e` / `ctrl-y` scroll it.

The mouse selects an issue or a field with a click and edits it with a
double-click. The wheel moves the cursor in the list, scrolls the details
under the pointer, and scrolls the field under the pointer in the form.
In the list, `ctrl-e` / `ctrl-y` also scroll the details. A click in the field in edit moves the text
cursor.
