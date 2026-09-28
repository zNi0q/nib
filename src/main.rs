use std::io::stdout;
use std::path::PathBuf;

use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, Event, KeyEventKind,
};
use crossterm::execute;
use nib::app::App;

const HELP: &str = "nib — a small terminal code editor

usage: nib [folder | file]

  nib            open the current folder
  nib <folder>   open a folder in the file tree
  nib <file>     open a file (creates it on first save if missing)";

fn main() -> std::io::Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("-h" | "--help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("nib {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    let path = PathBuf::from(arg.unwrap_or_else(|| ".".into()));
    if !path.exists() && !path.parent().is_none_or(|p| p.as_os_str().is_empty() || p.is_dir()) {
        eprintln!("nib: folder {} does not exist", path.parent().unwrap().display());
        std::process::exit(1);
    }
    let mut app = App::new(&path);

    let mut term = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste, EnableFocusChange)?;
    let res = (|| -> std::io::Result<()> {
        while !app.quit {
            term.draw(|f| nib::ui::draw(f, &mut app))?;
            // Block until input; only wake on a timer when an auto-save is pending.
            let ready = match app.autosave_wait() {
                Some(wait) => event::poll(wait)?,
                None => true,
            };
            if ready {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => app.on_key(k),
                    Event::Paste(s) => app.on_paste(&s),
                    Event::Mouse(m) => app.on_mouse(m),
                    Event::FocusLost => app.flush(),
                    _ => {}
                }
            }
            app.tick();
        }
        Ok(())
    })();
    // Never lose edits, even if the terminal went away.
    app.flush();
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
    ratatui::restore();
    res
}
