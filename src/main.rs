//! backlog: a terminal UI for the issues in a `.backlog.md` file.

mod app;
mod backlog;
mod input;
mod markdown;
mod prompt;
mod textarea;
mod ui;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;
use std::{env, fs};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use app::{App, Effect};
use backlog::{Backlog, FILE_NAME};

const USAGE: &str = "\
usage: backlog [FILE]
       backlog -V | --version

Manage the issues in FILE. Without FILE, use the nearest .backlog.md
in the current directory or one of its parents.
Press ? in the program for the key bindings.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("backlog: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let path = match env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("backlog {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(arg) => PathBuf::from(arg),
        None => {
            let cwd = env::current_dir()?;
            Backlog::find(&cwd).unwrap_or_else(|| cwd.join(FILE_NAME))
        }
    };

    if !path.exists() {
        if !ask(&format!(
            "{} does not exist. Create it? [y/N] ",
            path.display()
        ))? {
            return Ok(());
        }
        Backlog::create(&path)?;
    }
    let path = path.canonicalize().unwrap_or(path);

    let mut app = App::new(Backlog::load(&path)?);
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    let result = event_loop(&mut terminal, &mut app);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn ask(question: &str) -> io::Result<bool> {
    print!("{question}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        // Wait for input, and check the file for changes at each timeout.
        if !event::poll(Duration::from_millis(500))? {
            if app.backlog.changed_on_disk() {
                app.reload();
            }
            continue;
        }
        let key = match event::read()? {
            Event::Key(key) => key,
            Event::Mouse(m) => {
                app.handle_mouse(m);
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.handle_key(key) {
            Effect::None => {}
            Effect::Quit => return Ok(()),
            Effect::Editor { text, single_line } => {
                let result = suspended(terminal, || edit_external(&text));
                let result = result.map(|t| if single_line { join_lines(&t) } else { t });
                app.editor_done(result);
            }
            Effect::Copy(text) => {
                app.message = Some(match copy_to_clipboard(&text) {
                    Ok(()) => format!("Copied: {text}"),
                    Err(e) => format!("Cannot copy: {e}"),
                });
            }
        }
    }
}

/// Leave the TUI, run `f` on the normal screen, and come back.
fn suspended<T>(terminal: &mut DefaultTerminal, f: impl FnOnce() -> T) -> T {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    let result = f();
    let _ = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);
    let _ = enable_raw_mode();
    let _ = terminal.clear();
    result
}

/// Edit `text` in $VISUAL or $EDITOR (default `vi`) and return the result.
fn edit_external(text: &str) -> io::Result<String> {
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|name| env::var(name).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    let file = env::temp_dir().join(format!("backlog-{}.md", std::process::id()));
    fs::write(&file, text)?;
    let result = run_editor(&editor, &file).and_then(|()| fs::read_to_string(&file));
    let _ = fs::remove_file(&file);
    Ok(result?.trim_end().to_string())
}

fn run_editor(editor: &str, file: &Path) -> io::Result<()> {
    // The shell splits $EDITOR, so that values such as "code -w" work.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(file)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{editor} exited with {status}")))
    }
}

/// Copy `text` to the clipboard with the OSC 52 escape sequence, which most
/// terminals support, also over SSH. Also give it to wl-copy, xclip or xsel
/// when a display server is available.
fn copy_to_clipboard(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()?;

    let tools: [(&str, &[&str], &str); 3] = [
        ("wl-copy", &[], "WAYLAND_DISPLAY"),
        ("xclip", &["-selection", "clipboard"], "DISPLAY"),
        ("xsel", &["--clipboard", "--input"], "DISPLAY"),
    ];
    for (cmd, args, display) in tools {
        if env::var_os(display).is_none() {
            continue;
        }
        let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        break;
    }
    Ok(())
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let byte = |i: usize| *chunk.get(i).unwrap_or(&0) as u32;
        let n = byte(0) << 16 | byte(1) << 8 | byte(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn join_lines(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
