# backlog

A terminal UI for the issues in a `.backlog.md` file.

Static Linux binaries for x86_64 and aarch64 are on the
[releases page](https://github.com/pglira/backlog/releases). To build from source:

```sh
cargo install --path .
backlog            # nearest .backlog.md in this directory or a parent
backlog FILE       # a specific file
```

If the file does not exist, `backlog` asks to create it.
When another program changes the file, `backlog` reads it again. Drag the
border between the list and the details with the mouse to resize them.

Each issue has an id, a title, a description, a "done when" condition,
notes, an outcome (a short summary of the work), and a status: `open`,
`active`, `done` or `cancelled`. The file is plain Markdown and is written
after each change. The fields show Markdown (headings, lists, task lists,
quotes, tables, code, **bold**, *italic*, links, images) rendered; a field in edit shows
the raw text:

```markdown
# Backlog

## #1 Fix login timeout
status: active

### Description
The session expires after 5 minutes.

### Done when
A user stays logged in for 8 hours.

### Notes
The timeout is set in `config/session.toml`.

### Outcome
The timeout is now 8 hours and comes from the configuration file.
```

## Images

In a field other than the title, `ctrl-v` pastes an image from the
clipboard (with `wl-paste` or `xclip`). A pasted path of an image file, for
example from a terminal that gives a dropped file or a clipboard image as a
path, also inserts the image. `backlog` copies the image to
`.backlog/images/` next to the backlog file and inserts a link such as
`![Image #1](.backlog/images/3f2a9c0d1b7e4a65.png)`. The file name is a
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
| `p`               | show / hide the details            |
| `S`               | settings                           |
| `/`               | search                             |
| `q`               | quit                               |

`S` opens the settings. `space` toggles the selected setting, and `h` /
`l` change a number. `backlog` saves each change at once in
`backlog/config.toml` in `$XDG_CONFIG_HOME` (default `~/.config`):

```toml
details = true       # show the details at start
split = 45           # size of the list, in percent (10 to 90)
hide_closed = false  # hide done and cancelled issues
newest_first = false # show the newest issues first
images = true        # show image previews
image_width = 20     # width of the image previews, in cells (10 to 60)
rounded = true       # rounded corners of the borders
```

A drag of the border between the list and the details also saves `split`.

`y` copies a prompt such as `Work on issue #3 "..." from the backlog file
/path/.backlog.md. ...` to the clipboard. The prompt also asks the agent to
set the issue to `active` when it starts, and never to set it to `done`
without your explicit request. Only when you ask for `done` does the agent
write a short summary of its work in the Outcome section, as structured
Markdown in ASD-STE100 Simplified Technical English. The prompt is
copied with the OSC 52 escape sequence and with `wl-copy`, `xclip` or
`xsel` when available.

An issue can have no title, but then it needs a description. The list
shows it as "(no title)" with the start of the description, and the prompt
asks the agent to give it a short title (20 characters at most).

`P` edits the prompt template in the same Vim editor as the fields; `esc`
in normal mode saves it. `E` edits the template in `$VISUAL` or `$EDITOR`
and saves it when the editor exits. `{id}`, `{title}` and `{path}` in the template are
replaced by the values of the issue. An edited template is stored in the
`## Agent prompt` section at the top of `.backlog.md`; an empty template, or
one equal to the built-in default, removes the section.

In the form, `i` edits a field inline and `e` edits it in `$VISUAL` or
`$EDITOR`. `w` saves. Inline editing has Vim modes: it starts in insert
mode, which also takes readline keys (`ctrl-a`, `ctrl-e`, `ctrl-w`,
`ctrl-k`, `alt-b`, ...). `esc` goes to normal mode (`hjkl`, `w`, `b`,
`dd`, `cw`, `p`, `u`, ...), and a second `esc` stops editing the field.
`ctrl-x ctrl-e` continues in `$EDITOR`.

The form shows the outcome in its right half. A field with more text than
space shows a scroll bar; the mouse wheel or `ctrl-e` / `ctrl-y` scroll it.

The mouse selects an issue or a field with a click and edits it with a
double-click. The wheel moves the cursor in the list and scrolls the field
under the pointer in the form. A click in the field in edit moves the text
cursor.
