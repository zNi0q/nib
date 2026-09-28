//! Editor state and input handling, independent of the terminal so it can be
//! unit-tested. `ui.rs` draws it; `main.rs` feeds it events.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::buffer::{char_at_col, display_col, Buffer, Pos};
use crate::highlight::{highlight_line, lang_for, State};
use crate::tree::Tree;

/// Auto-save runs this long after the last edit.
pub const AUTOSAVE_DELAY: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Editor,
}

#[derive(Clone, Debug, PartialEq)]
pub enum After {
    Quit,
    Close,
    Open(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// "Save changes?" before doing `After`.
    Unsaved(After),
    Find(String),
    /// Command palette: filter text and selected row.
    Palette { query: String, sel: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Palette,
    Save,
    Close,
    Quit,
    Find,
    Undo,
    Redo,
    CutLine,
    PasteLine,
    ToggleSidebar,
    SwitchFocus,
    Refresh,
    ToggleAutosave,
}

pub struct Command {
    pub name: &'static str,
    /// Ctrl+<key> shortcut, if any.
    pub ctrl: Option<char>,
    pub cmd: Cmd,
}

/// Every action, in the order the palette lists them.
pub const COMMANDS: &[Command] = &[
    Command { name: "Save file", ctrl: Some('s'), cmd: Cmd::Save },
    Command { name: "Find in file", ctrl: Some('f'), cmd: Cmd::Find },
    Command { name: "Undo", ctrl: Some('z'), cmd: Cmd::Undo },
    Command { name: "Redo", ctrl: Some('y'), cmd: Cmd::Redo },
    Command { name: "Cut line", ctrl: Some('k'), cmd: Cmd::CutLine },
    Command { name: "Paste line", ctrl: Some('u'), cmd: Cmd::PasteLine },
    Command { name: "Switch tree / editor", ctrl: Some('e'), cmd: Cmd::SwitchFocus },
    Command { name: "Toggle sidebar", ctrl: Some('b'), cmd: Cmd::ToggleSidebar },
    Command { name: "Refresh file tree", ctrl: Some('r'), cmd: Cmd::Refresh },
    Command { name: "Toggle auto-save", ctrl: None, cmd: Cmd::ToggleAutosave },
    Command { name: "Close file", ctrl: Some('w'), cmd: Cmd::Close },
    Command { name: "Quit nib", ctrl: Some('q'), cmd: Cmd::Quit },
    Command { name: "Command palette", ctrl: Some('p'), cmd: Cmd::Palette },
];

pub struct App {
    pub tree: Tree,
    pub buf: Option<Buffer>,
    pub focus: Focus,
    pub show_tree: bool,
    pub prompt: Option<Prompt>,
    pub status: String,
    pub quit: bool,
    /// Save automatically after edits, and before switching/closing/quitting.
    pub autosave: bool,
    last_edit: Option<Instant>,
    /// Buffer version whose auto-save failed; don't retry until the next edit.
    autosave_failed: Option<u64>,
    /// First visible line and display column of the editor.
    pub scroll: (usize, usize),
    /// Screen areas from the last draw, used for mouse clicks.
    pub tree_area: Rect,
    pub text_area: Rect,
    clipboard: String,
    last_find: String,
    /// Highlighter state at the start of each line, for buffer `states_version`.
    states: Vec<State>,
    states_version: Option<u64>,
}

impl App {
    /// `path` may be a folder, an existing file, or a new file to create.
    pub fn new(path: &Path) -> App {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let (root, file) = if path.is_dir() {
            (path, None)
        } else {
            (path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/")), Some(path))
        };
        let mut app = App {
            tree: Tree::new(root),
            buf: None,
            focus: Focus::Tree,
            show_tree: true,
            prompt: None,
            status: String::new(),
            quit: false,
            autosave: true,
            last_edit: None,
            autosave_failed: None,
            scroll: (0, 0),
            tree_area: Rect::default(),
            text_area: Rect::default(),
            clipboard: String::new(),
            last_find: String::new(),
            states: Vec::new(),
            states_version: None,
        };
        if let Some(f) = file {
            if f.exists() {
                app.open(&f);
            } else {
                app.buf = Some(Buffer::empty(Some(f.clone())));
                app.focus = Focus::Editor;
                app.status = format!("New file: {}", app.rel(&f));
            }
            app.tree.reveal(&f);
        }
        app
    }

    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.tree.root).unwrap_or(p).display().to_string()
    }

    /// Highlighter state at the start of each line. After an edit only lines
    /// from the first changed one are redone, stopping as soon as the state
    /// matches what it was before (so typing costs one line, not the file).
    pub fn line_states(&mut self) -> &[State] {
        let buf = self.buf.as_mut().unwrap();
        let Some(lang) = buf.path.as_deref().and_then(lang_for) else {
            self.states.clear();
            return &self.states;
        };
        if self.states_version == Some(buf.version) {
            return &self.states;
        }
        let old = std::mem::take(&mut self.states);
        let from = if self.states_version.is_none() { 0 } else { buf.changed_from.min(old.len()) };
        buf.changed_from = usize::MAX;
        self.states_version = Some(buf.version);

        let n = buf.lines.len();
        let same_len = old.len() == n;
        let mut st = old.get(from).copied().unwrap_or_default();
        let mut states = old[..from].to_vec();
        for y in from..n {
            if same_len && y > from && old[y] == st {
                states.extend_from_slice(&old[y..]);
                break;
            }
            states.push(st);
            highlight_line(&buf.lines[y], lang, &mut st);
        }
        self.states = states;
        &self.states
    }

    fn open(&mut self, path: &Path) {
        match Buffer::open(path) {
            Ok(b) => {
                self.buf = Some(b);
                self.states_version = None;
                self.scroll = (0, 0);
                self.focus = Focus::Editor;
                self.status = format!("Opened {}", self.rel(path));
            }
            Err(e) => self.status = format!("Can't open {}: {e}", self.rel(path)),
        }
    }

    fn save(&mut self) -> bool {
        let Some(path) = self.buf.as_ref().map(|b| b.path.clone()) else { return false };
        let name = path.map(|p| self.rel(&p)).unwrap_or_default();
        let is_new = self.buf.as_ref().unwrap().path.as_ref().is_some_and(|p| !p.exists());
        match self.buf.as_mut().unwrap().save() {
            Ok(()) => {
                self.status = format!("Saved {name}");
                if is_new {
                    self.tree.refresh();
                }
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                false
            }
        }
    }

    /// Run `after`, asking first if there are unsaved changes.
    fn request(&mut self, after: After) {
        if self.buf.as_ref().is_some_and(|b| b.dirty) && !(self.autosave && self.save()) {
            self.prompt = Some(Prompt::Unsaved(after));
        } else {
            self.run(after);
        }
    }

    fn run(&mut self, after: After) {
        match after {
            After::Quit => self.quit = true,
            After::Close => {
                self.buf = None;
                self.focus = Focus::Tree;
                self.show_tree = true;
            }
            After::Open(p) => self.open(&p),
        }
    }

    /// Commands matching the palette filter: every character of `query`
    /// must appear in order in the name (case-insensitive).
    pub fn palette_matches(query: &str) -> Vec<&'static Command> {
        let q = query.to_lowercase();
        COMMANDS
            .iter()
            .filter(|c| {
                let mut name = c.name.to_lowercase().into_bytes().into_iter();
                q.bytes().all(|b| name.any(|n| n == b))
            })
            .collect()
    }

    pub fn exec(&mut self, cmd: Cmd) {
        let needs_file = matches!(cmd, Cmd::Close | Cmd::Find | Cmd::Undo | Cmd::Redo | Cmd::CutLine | Cmd::PasteLine);
        if needs_file && self.buf.is_none() {
            self.status = "Open a file first".into();
            return;
        }
        match cmd {
            Cmd::Palette => self.prompt = Some(Prompt::Palette { query: String::new(), sel: 0 }),
            Cmd::Save => { self.save(); }
            Cmd::Close => self.request(After::Close),
            Cmd::Quit => self.request(After::Quit),
            Cmd::Find => self.prompt = Some(Prompt::Find(self.last_find.clone())),
            Cmd::Undo => { if !self.buf.as_mut().unwrap().undo() { self.status = "Nothing to undo".into() } }
            Cmd::Redo => { if !self.buf.as_mut().unwrap().redo() { self.status = "Nothing to redo".into() } }
            Cmd::CutLine => self.clipboard = self.buf.as_mut().unwrap().cut_line(),
            Cmd::PasteLine => {
                if self.clipboard.is_empty() {
                    self.status = "Nothing to paste (Cut line first)".into();
                } else {
                    let clip = self.clipboard.clone();
                    self.buf.as_mut().unwrap().paste_lines(&clip);
                }
            }
            Cmd::ToggleSidebar => {
                self.show_tree = !self.show_tree || self.buf.is_none();
                if !self.show_tree { self.focus = Focus::Editor }
            }
            Cmd::SwitchFocus => {
                self.focus = match self.focus {
                    Focus::Tree if self.buf.is_some() => Focus::Editor,
                    _ => { self.show_tree = true; Focus::Tree }
                };
            }
            Cmd::Refresh => { self.tree.refresh(); self.status = "Refreshed file tree".into(); }
            Cmd::ToggleAutosave => {
                self.autosave = !self.autosave;
                self.status = format!("Auto-save {}", if self.autosave { "on" } else { "off" });
            }
        }
    }

    fn version(&self) -> Option<u64> {
        self.buf.as_ref().map(|b| b.version)
    }

    /// Restart the auto-save countdown if the last input changed the text.
    fn note_edit(&mut self, before: Option<u64>) {
        if self.version() != before && self.buf.as_ref().is_some_and(|b| b.dirty) {
            self.last_edit = Some(Instant::now());
        }
    }

    /// How long until an auto-save is due; `None` when nothing is pending,
    /// so the main loop can sleep until the next key press.
    pub fn autosave_wait(&self) -> Option<Duration> {
        let b = self.buf.as_ref()?;
        if !self.autosave || !b.dirty || self.autosave_failed == Some(b.version) || self.prompt.is_some() {
            return None;
        }
        Some(AUTOSAVE_DELAY.saturating_sub(self.last_edit?.elapsed()))
    }

    /// Called by the main loop when it wakes up: save if the delay has passed.
    pub fn tick(&mut self) {
        if self.autosave_wait() == Some(Duration::ZERO) {
            self.flush();
        }
    }

    /// Save now if auto-save is on and there are changes (e.g. the terminal
    /// lost focus or nib is exiting).
    pub fn flush(&mut self) {
        let Some(b) = self.buf.as_ref() else { return };
        if self.autosave && b.dirty && self.autosave_failed != Some(b.version) {
            let v = b.version;
            if !self.save() {
                self.autosave_failed = Some(v);
                self.status = format!("Auto-save failed: {}", self.status.trim_start_matches("Save failed: "));
            }
        }
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        let before = self.version();
        self.handle_key(k);
        self.note_edit(before);
    }

    fn handle_key(&mut self, k: KeyEvent) {
        self.status.clear();
        if self.prompt.is_some() {
            return self.on_prompt_key(k);
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(c) = k.code {
                if let Some(cmd) = COMMANDS.iter().find(|cmd| cmd.ctrl == Some(c)) {
                    return self.exec(cmd.cmd);
                }
                if c == 'c' {
                    self.status = "Use Ctrl+Q to quit".into();
                    return;
                }
            }
        }
        match self.focus {
            Focus::Tree => self.tree_key(k),
            Focus::Editor => self.editor_key(k),
        }
    }

    fn on_prompt_key(&mut self, k: KeyEvent) {
        match self.prompt.take().unwrap() {
            Prompt::Unsaved(after) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if self.save() { self.run(after) }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => self.run(after),
                KeyCode::Esc | KeyCode::Char('c') => self.status = "Cancelled".into(),
                _ => self.prompt = Some(Prompt::Unsaved(after)),
            },
            Prompt::Palette { mut query, mut sel } => {
                let n = Self::palette_matches(&query).len();
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        if let Some(c) = Self::palette_matches(&query).get(sel) {
                            self.exec(c.cmd);
                        }
                        return;
                    }
                    KeyCode::Up => sel = sel.saturating_sub(1),
                    KeyCode::Down => sel = (sel + 1).min(n.saturating_sub(1)),
                    KeyCode::Backspace => { query.pop(); sel = 0; }
                    KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => { query.push(c); sel = 0; }
                    _ => {}
                }
                self.prompt = Some(Prompt::Palette { query, sel });
            }
            Prompt::Find(mut q) => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let found = self.buf.as_mut().unwrap().find_next(&q);
                    self.status = if found { String::new() } else { format!("Not found: {q}") };
                    self.last_find = q.clone();
                    self.prompt = Some(Prompt::Find(q));
                }
                KeyCode::Backspace => { q.pop(); self.prompt = Some(Prompt::Find(q)); }
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    q.push(c);
                    self.prompt = Some(Prompt::Find(q));
                }
                _ => self.prompt = Some(Prompt::Find(q)),
            },
        }
    }

    fn activate_tree_item(&mut self) {
        let Some(e) = self.tree.selected().cloned() else { return };
        if e.is_dir {
            self.tree.toggle(self.tree.sel);
        } else if self.buf.as_ref().and_then(|b| b.path.as_ref()) == Some(&e.path) {
            self.focus = Focus::Editor;
        } else {
            self.request(After::Open(e.path));
        }
    }

    fn tree_key(&mut self, k: KeyEvent) {
        let page = self.tree_area.height.max(2) as isize - 1;
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.tree.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.tree.move_sel(1),
            KeyCode::PageUp => self.tree.move_sel(-page),
            KeyCode::PageDown => self.tree.move_sel(page),
            KeyCode::Home => self.tree.move_sel(isize::MIN / 2),
            KeyCode::End => self.tree.move_sel(isize::MAX / 2),
            KeyCode::Enter | KeyCode::Char('l') => self.activate_tree_item(),
            KeyCode::Right => {
                if self.tree.selected().is_some_and(|e| e.is_dir) {
                    self.tree.expand(self.tree.sel);
                } else {
                    self.activate_tree_item();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.tree.selected().is_some_and(|e| e.expanded) {
                    self.tree.collapse(self.tree.sel);
                } else {
                    self.tree.select_parent();
                }
            }
            KeyCode::Esc | KeyCode::Tab if self.buf.is_some() => self.focus = Focus::Editor,
            _ => {}
        }
    }

    fn page(&self) -> usize {
        (self.text_area.height as usize).saturating_sub(1).max(1)
    }

    fn editor_key(&mut self, k: KeyEvent) {
        let page = self.page();
        let Some(b) = self.buf.as_mut() else {
            if k.code == KeyCode::Esc { self.focus = Focus::Tree }
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Char(c) if !ctrl && !alt => b.insert(&c.to_string()),
            KeyCode::Enter => b.newline(),
            KeyCode::Tab => b.tab(),
            KeyCode::Backspace => b.backspace(),
            KeyCode::Delete => b.delete_forward(),
            KeyCode::Left if ctrl => b.word_left(),
            KeyCode::Right if ctrl => b.word_right(),
            KeyCode::Left => b.left(),
            KeyCode::Right => b.right(),
            KeyCode::Up => b.up(1),
            KeyCode::Down => b.down(1),
            KeyCode::PageUp => b.up(page),
            KeyCode::PageDown => b.down(page),
            KeyCode::Home if ctrl => b.doc_start(),
            KeyCode::End if ctrl => b.doc_end(),
            KeyCode::Home => b.home(),
            KeyCode::End => b.end(),
            KeyCode::Esc => { self.show_tree = true; self.focus = Focus::Tree }
            _ => {}
        }
    }

    /// Bracketed paste from the terminal: insert verbatim (no auto-indent).
    pub fn on_paste(&mut self, text: &str) {
        if self.prompt.is_some() || self.focus != Focus::Editor {
            return;
        }
        let before = self.version();
        if let Some(b) = self.buf.as_mut() {
            b.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
        }
        self.note_edit(before);
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let hit = |r: Rect| m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height;
        let in_tree = self.show_tree && hit(self.tree_area);
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                if in_tree {
                    self.tree.move_sel(if up { -3 } else { 3 });
                } else if let Some(b) = self.buf.as_mut() {
                    if up { b.up(3) } else { b.down(3) }
                }
            }
            MouseEventKind::Down(MouseButton::Left) if self.prompt.is_none() => {
                if in_tree {
                    let i = self.tree.scroll + (m.row - self.tree_area.y) as usize;
                    if i < self.tree.items.len() {
                        self.tree.sel = i;
                        self.focus = Focus::Tree;
                        self.activate_tree_item();
                    }
                } else if hit(self.text_area) {
                    if let Some(b) = self.buf.as_mut() {
                        let y = self.scroll.0 + (m.row - self.text_area.y) as usize;
                        let y = y.min(b.lines.len() - 1);
                        let col = self.scroll.1 + (m.column - self.text_area.x) as usize;
                        b.set_cur(Pos { y, x: char_at_col(&b.lines[y], col) });
                        self.focus = Focus::Editor;
                    }
                }
            }
            _ => {}
        }
    }

    /// Scroll the editor and tree so the cursor/selection is on screen.
    pub fn follow_cursor(&mut self) {
        let h = self.tree_area.height as usize;
        if h > 0 {
            let t = &mut self.tree;
            if t.sel < t.scroll { t.scroll = t.sel }
            if t.sel >= t.scroll + h { t.scroll = t.sel + 1 - h }
        }
        let (Some(b), h, w) = (self.buf.as_ref(), self.text_area.height as usize, self.text_area.width as usize) else { return };
        if h == 0 || w == 0 {
            return;
        }
        let (mut sy, mut sx) = self.scroll;
        let y = b.cur.y;
        if y < sy { sy = y }
        if y >= sy + h { sy = y + 1 - h }
        let col = display_col(&b.lines[y], b.cur.x);
        if col < sx { sx = col }
        if col >= sx + w { sx = col + 1 - w }
        self.scroll = (sy, sx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn typ(app: &mut App, s: &str) {
        for c in s.chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
    }

    fn project(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nib-app-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src")).unwrap();
        fs::write(d.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(d.join("notes.txt"), "hi\n").unwrap();
        d
    }

    #[test]
    fn open_edit_save_and_unsaved_prompts() {
        let d = project("flow");
        let mut app = App::new(&d);
        app.autosave = false;
        assert_eq!(app.focus, Focus::Tree);
        app.on_key(key(KeyCode::Enter)); // expand src
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter)); // open main.rs
        assert_eq!(app.focus, Focus::Editor);
        typ(&mut app, "// ");
        app.on_key(ctrl('s'));
        assert_eq!(fs::read_to_string(d.join("src/main.rs")).unwrap(), "// fn main() {}\n");

        typ(&mut app, "x");
        app.on_key(ctrl('w'));
        assert_eq!(app.prompt, Some(Prompt::Unsaved(After::Close)));
        app.on_key(key(KeyCode::Esc));
        assert!(app.prompt.is_none() && app.buf.is_some(), "Esc cancels");
        app.on_key(ctrl('q'));
        app.on_key(key(KeyCode::Char('n')));
        assert!(app.quit);
        assert_eq!(fs::read_to_string(d.join("src/main.rs")).unwrap(), "// fn main() {}\n", "discarded");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn switching_files_asks_to_save_and_y_saves() {
        let d = project("switch");
        let mut app = App::new(&d.join("notes.txt"));
        app.autosave = false;
        assert_eq!(app.tree.selected().unwrap().name, "notes.txt");
        typ(&mut app, "A");
        app.on_key(key(KeyCode::Esc)); // to tree
        app.on_key(key(KeyCode::Home));
        app.on_key(key(KeyCode::Enter)); // collapse/expand src
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter)); // main.rs
        assert_eq!(app.prompt, Some(Prompt::Unsaved(After::Open(d.join("src/main.rs")))));
        app.on_key(key(KeyCode::Char('y')));
        assert_eq!(fs::read_to_string(d.join("notes.txt")).unwrap(), "Ahi\n");
        assert!(app.buf.as_ref().unwrap().path.as_ref().unwrap().ends_with("src/main.rs"));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn new_file_find_undo_paste() {
        let d = project("new");
        let p = d.join("new.go");
        let mut app = App::new(&p);
        assert!(app.status.starts_with("New file"));
        app.on_paste("package main\r\nfunc main() {}\r\n");
        app.on_key(ctrl('s'));
        assert_eq!(fs::read_to_string(&p).unwrap(), "package main\nfunc main() {}\n\n");
        assert!(app.tree.items.iter().any(|e| e.name == "new.go"));

        app.on_key(ctrl('f'));
        typ(&mut app, "main");
        app.on_key(key(KeyCode::Enter));
        let b = app.buf.as_ref().unwrap();
        assert_eq!(b.cur, Pos { y: 0, x: 8 });
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.buf.as_ref().unwrap().cur, Pos { y: 1, x: 5 });
        app.on_key(key(KeyCode::Esc));
        assert!(app.prompt.is_none());

        app.on_key(ctrl('z'));
        assert_eq!(app.buf.as_ref().unwrap().text(), "\n");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn palette_filters_and_runs_commands() {
        assert_eq!(App::palette_matches("sav")[0].cmd, Cmd::Save);
        assert_eq!(App::palette_matches("qn")[0].cmd, Cmd::Quit, "subsequence match");
        assert_eq!(App::palette_matches("").len(), COMMANDS.len());
        assert!(App::palette_matches("zzz").is_empty());

        let d = project("palette");
        let mut app = App::new(&d.join("notes.txt"));
        typ(&mut app, "X");
        app.on_key(ctrl('p'));
        assert!(matches!(app.prompt, Some(Prompt::Palette { .. })));
        typ(&mut app, "save");
        app.on_key(key(KeyCode::Enter));
        assert!(app.prompt.is_none());
        assert_eq!(fs::read_to_string(d.join("notes.txt")).unwrap(), "Xhi\n");

        // Arrow keys pick a row; typing in the palette never reaches the file.
        app.on_key(ctrl('p'));
        typ(&mut app, "u");
        app.on_key(key(KeyCode::Up));
        app.on_key(key(KeyCode::Enter)); // "u" matches Undo first
        assert_eq!(app.buf.as_ref().unwrap().text(), "hi\n", "Undo ran");
        app.on_key(ctrl('p'));
        app.on_key(key(KeyCode::Esc));
        assert!(app.prompt.is_none() && !app.quit);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn incremental_highlight_matches_full_recompute() {
        use crate::highlight::line_states;
        let d = project("hl");
        let p = d.join("a.vue");
        fs::write(&p, "<template>\n  <p>{{ x }}</p>\n</template>\n<script>\nconst a = 1\n</script>\n<style>\n.a { color: red }\n</style>\n").unwrap();
        let mut app = App::new(&p);
        let full = |app: &App| line_states(&app.buf.as_ref().unwrap().lines, lang_for(&p).unwrap());
        app.line_states();
        // Edits that change later lines' state: open a comment, a string, a tag.
        for (y, text) in [(0, "<!-- "), (4, "`"), (1, "<div "), (7, "/* ")] {
            app.buf.as_mut().unwrap().set_cur(Pos { y, x: 0 });
            app.on_paste(text);
            assert_eq!(app.line_states().to_vec(), full(&app), "after inserting {text:?} on line {y}");
            app.on_key(ctrl('z'));
            assert_eq!(app.line_states().to_vec(), full(&app), "after undo of {text:?}");
        }
        app.on_paste("\n\n");
        assert_eq!(app.line_states().to_vec(), full(&app));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn autosave_after_delay_and_before_switching_or_quitting() {
        let d = project("autosave");
        let notes = d.join("notes.txt");
        let mut app = App::new(&notes);
        assert!(app.autosave_wait().is_none(), "nothing pending when clean");
        typ(&mut app, "A");
        let wait = app.autosave_wait().unwrap();
        assert!(wait > Duration::ZERO && wait <= AUTOSAVE_DELAY);
        app.tick();
        assert_eq!(fs::read_to_string(&notes).unwrap(), "hi\n", "not before the delay");
        std::thread::sleep(AUTOSAVE_DELAY);
        app.tick();
        assert_eq!(fs::read_to_string(&notes).unwrap(), "Ahi\n");
        assert!(app.autosave_wait().is_none());

        // Switching files saves first instead of asking.
        typ(&mut app, "B");
        app.on_key(key(KeyCode::Esc));
        app.tree.sel = app.tree.items.iter().position(|e| e.name == "src").unwrap();
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter));
        assert!(app.prompt.is_none());
        assert_eq!(fs::read_to_string(&notes).unwrap(), "ABhi\n");

        // Quitting saves too.
        typ(&mut app, "//");
        app.on_key(ctrl('q'));
        assert!(app.quit && app.prompt.is_none());
        assert_eq!(fs::read_to_string(d.join("src/main.rs")).unwrap(), "//fn main() {}\n");

        // Undo still works after auto-saves.
        app.on_key(ctrl('z'));
        assert_eq!(app.buf.as_ref().unwrap().text(), "fn main() {}\n");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn failed_autosave_reports_once_and_falls_back_to_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let d = project("ro");
        let f = d.join("notes.txt");
        let mut app = App::new(&f);
        fs::set_permissions(&f, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        typ(&mut app, "A");
        app.flush();
        assert!(app.status.starts_with("Auto-save failed"), "{}", app.status);
        assert!(app.autosave_wait().is_none(), "no retry loop");
        app.on_key(ctrl('q'));
        assert!(matches!(app.prompt, Some(Prompt::Unsaved(After::Quit))), "asks instead of losing the edit");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn binary_file_is_refused_not_opened() {
        let d = project("bin");
        fs::write(d.join("img.png"), [0x89, b'P', 0, 0]).unwrap();
        let mut app = App::new(&d);
        let i = app.tree.items.iter().position(|e| e.name == "img.png").unwrap();
        app.tree.sel = i;
        app.on_key(key(KeyCode::Enter));
        assert!(app.buf.is_none());
        assert_eq!(app.status, "Can't open img.png: binary file");
        fs::remove_dir_all(d).unwrap();
    }
}
