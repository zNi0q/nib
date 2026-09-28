use std::io::stdout;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, Event, KeyEventKind,
};
use crossterm::execute;
use nib::app::App;
use nib::lsp::{Lsp, Wake};

const HELP: &str = "nib — a small terminal code editor

usage: nib [folder | file]

  nib            open the current folder
  nib <folder>   open a folder in the file tree
  nib <file>     open a file (creates it on first save if missing)
  nib plugin     manage language-server plugins (list, add, new, remove)
  nib config     create/check the config file (~/.config/nib/config.nib)";

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("plugin") => std::process::exit(nib::plugin::cli(&args[1..])),
        Some("config") => std::process::exit(nib::config::cli(&args[1..])),
        _ => {}
    }
    let arg = args.first().cloned();
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
    let (cfg, config_errors) = nib::config::load();
    let lsp_enabled = cfg.settings.lsp_enabled;
    let idle = cfg.settings.lsp_idle_timeout;
    app.apply_config(cfg);

    let mut term = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste, EnableFocusChange)?;

    // Keyboard input and language-server messages arrive on one channel, so
    // the loop sleeps until something happens (or a timer is due).
    let (tx, rx) = mpsc::channel();
    let input = tx.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if input.send(Wake::Term(ev)).is_err() {
                break;
            }
        }
    });
    let mut warnings = config_errors;
    if lsp_enabled {
        let (plugins, plugin_warnings) = nib::plugin::load();
        let mut lsp = Lsp::new(plugins, tx);
        lsp.set_default_idle(idle);
        app.attach_lsp(lsp);
        warnings.extend(plugin_warnings);
    }
    if let Some(w) = warnings.first() {
        app.status = w.clone();
    }

    let res = (|| -> std::io::Result<()> {
        while !app.quit {
            term.draw(|f| nib::ui::draw(f, &mut app))?;
            let first = match app.next_wake() {
                Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                    Ok(w) => Some(w),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(w) => Some(w),
                    Err(_) => break,
                },
            };
            // Handle everything already queued before redrawing once.
            for w in first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())) {
                match w {
                    Wake::Term(Event::Key(k)) if k.kind != KeyEventKind::Release => app.on_key(k),
                    Wake::Term(Event::Paste(s)) => app.on_paste(&s),
                    Wake::Term(Event::Mouse(m)) => app.on_mouse(m),
                    Wake::Term(Event::FocusLost) => app.flush(),
                    Wake::Term(_) => {}
                    Wake::Lsp(id, msg) => app.on_lsp(id, msg),
                }
                if app.quit {
                    break;
                }
            }
            app.tick();
        }
        Ok(())
    })();
    // Never lose edits, even if the terminal went away.
    app.flush();
    // Dropping the client kills every server right away.
    app.lsp = None;
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
    ratatui::restore();
    res
}
