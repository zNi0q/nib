use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use crossterm::event::{self, Event, KeyEventKind};
use nib::app::App;
use nib::lsp::{Lsp, Wake};

const HELP: &str = "nib — a small terminal code editor

usage: nib [folder | file]

  nib            open the current folder
  nib <folder>   open a folder in the file tree
  nib <file>     open a file (creates it on first save if missing)
  nib plugin     manage language-server plugins (list, add, new, remove)
  nib config     create/check the config file (~/.config/nib/config.nib)";

mod signals {
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::mpsc::Sender;

    use nib::lsp::Wake;

    static PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

    unsafe extern "C" {
        fn pipe(fds: *mut i32) -> i32;
        fn read(fd: i32, buf: *mut u8, n: usize) -> isize;
        fn write(fd: i32, buf: *const u8, n: usize) -> isize;
        fn signal(sig: i32, handler: usize) -> usize;
    }

    extern "C" fn on_signal(_: i32) {
        unsafe { write(PIPE_WRITE.load(Ordering::Relaxed), [1u8].as_ptr(), 1) };
    }

    pub fn watch(tx: Sender<Wake>) {
        let mut fds = [0i32; 2];
        unsafe {
            if pipe(fds.as_mut_ptr()) != 0 {
                return;
            }
            PIPE_WRITE.store(fds[1], Ordering::Relaxed);
            const SIGHUP: i32 = 1;
            const SIGTERM: i32 = 15;
            signal(SIGHUP, on_signal as extern "C" fn(i32) as usize);
            signal(SIGTERM, on_signal as extern "C" fn(i32) as usize);
        }
        std::thread::Builder::new()
            .stack_size(16 * 1024)
            .spawn(move || {
                let mut b = 0u8;
                if unsafe { read(fds[0], &mut b, 1) } > 0 {
                    let _ = tx.send(Wake::Closed);
                }
            })
            .ok();
    }
}

mod terminal {
    use std::io::stdout;

    use crossterm::event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
        EnableMouseCapture,
    };
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
    use crossterm::{cursor, execute};
    use nib::screen::Screen;

    pub fn enter() -> std::io::Result<Screen> {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave();
            hook(info);
        }));
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste, EnableFocusChange)?;
        let (w, h) = crossterm::terminal::size()?;
        Ok(Screen::new(w, h))
    }

    pub fn leave() {
        let _ = execute!(
            stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            DisableFocusChange,
            LeaveAlternateScreen,
            cursor::Show
        );
        let _ = disable_raw_mode();
    }
}

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

    let mut screen = terminal::enter()?;
    let mut out = std::io::stdout();

    let (tx, rx) = mpsc::channel();
    let input = tx.clone();
    signals::watch(tx.clone());
    std::thread::Builder::new().stack_size(64 * 1024).spawn(move || loop {
        match event::read() {
            Ok(ev) => {
                if input.send(Wake::Term(ev)).is_err() {
                    break;
                }
            }
            Err(_) => {
                let _ = input.send(Wake::Closed);
                break;
            }
        }
    })?;
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
            screen.begin();
            nib::ui::draw(&mut screen, &mut app);
            screen.flush(&mut out)?;
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
            for w in first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())) {
                match w {
                    Wake::Term(Event::Key(k)) if k.kind != KeyEventKind::Release => app.on_key(k),
                    Wake::Term(Event::Paste(s)) => app.on_paste(&s),
                    Wake::Term(Event::Mouse(m)) => app.on_mouse(m),
                    Wake::Term(Event::FocusLost) => app.flush(),
                    Wake::Term(Event::Resize(w, h)) => screen.resize(w, h),
                    Wake::Term(_) => {}
                    Wake::Lsp(id, msg) => app.on_lsp(id, msg),
                    Wake::Closed => app.quit = true,
                }
                if app.quit {
                    break;
                }
            }
            app.tick();
        }
        Ok(())
    })();
    app.flush();
    app.lsp = None;
    terminal::leave();
    res
}
