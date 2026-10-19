//! backlog: a terminal UI for the issues in a `.backlog.md` file.

mod app;
mod backlog;
mod input;
mod ui;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
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

fn join_lines(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
