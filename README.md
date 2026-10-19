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

Each issue has an id, a title, a description, a "done when" condition,
notes, and a status: `open`, `active`, `done` or `cancelled`. The file is
plain Markdown and is written after each change:

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
```

## Keys

Press `?` for all keys.

| List              |                              |
|-------------------|------------------------------|
| `j` `k` `g` `G`   | move                         |
| `a`               | add                          |
| `enter`           | edit                         |
| `x` `s` `c`       | toggle done / active / cancelled |
| `D`               | delete                       |
| `/`               | search                       |
| `q`               | quit                         |

In the form, `i` edits a field inline with readline keys (`ctrl-a`,
`ctrl-e`, `ctrl-w`, `ctrl-k`, `alt-b`, ...), `e` edits it in `$VISUAL`
or `$EDITOR`, and `w` saves. While editing inline, `ctrl-x ctrl-e`
continues in the editor. Multi-line fields always open in the editor.
