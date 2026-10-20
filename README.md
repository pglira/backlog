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
after each change:

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

## Keys

Press `?` for all keys.

| List              |                                   |
|-------------------|-----------------------------------|
| `j` `k` `gg` `G`  | move                              |
| `a` `o`           | add                               |
| `enter`           | edit                              |
| `x` `s` `c`       | toggle done / active / cancelled  |
| `dd`              | delete                            |
| `y`               | copy a prompt for an agent        |
| `p`               | show / hide the details           |
| `/`               | search                            |
| `q`               | quit                              |

`y` copies a prompt such as `Work on issue #3 "..." from the backlog file
/path/.backlog.md. ...` to the clipboard. The prompt also asks the agent to
write a short summary of its work in the Outcome section. The prompt is
copied with the OSC 52 escape sequence and with `wl-copy`, `xclip` or
`xsel` when available.

In the form, `i` edits a field inline and `e` edits it in `$VISUAL` or
`$EDITOR`. `w` saves. Inline editing has Vim modes: it starts in insert
mode, which also takes readline keys (`ctrl-a`, `ctrl-e`, `ctrl-w`,
`ctrl-k`, `alt-b`, ...). `esc` goes to normal mode (`hjkl`, `w`, `b`,
`dd`, `cw`, `p`, `u`, ...), and a second `esc` stops editing the field.
`ctrl-x ctrl-e` continues in `$EDITOR`.

The mouse selects an issue or a field with a click and edits it with a
double-click. The wheel moves the cursor. A click in the field in edit
moves the text cursor.
