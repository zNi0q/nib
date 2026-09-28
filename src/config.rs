use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use toml::{Table, Value};

use crate::app::{Cmd, COMMANDS};
use crate::theme::{self, Theme};

pub const FILE: &str = "config.nib";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub tab_width: usize,
    pub autosave: bool,
    pub autosave_delay: u64,
    pub sidebar_width: u16,
    pub icons: bool,
    pub line_numbers: bool,
    pub lsp_enabled: bool,
    pub lsp_idle_timeout: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tab_width: 4,
            autosave: true,
            autosave_delay: 1000,
            sidebar_width: 32,
            icons: true,
            line_numbers: true,
            lsp_enabled: true,
            lsp_idle_timeout: 120,
        }
    }
}

pub struct Config {
    pub settings: Settings,
    pub theme: Theme,
    pub keymap: Keymap,
}

impl Default for Config {
    fn default() -> Self {
        Config { settings: Settings::default(), theme: theme::TOKYONIGHT, keymap: Keymap::defaults() }
    }
}

pub fn dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/")).join(".config"))
        .join("nib")
}

pub fn path() -> PathBuf {
    dir().join(FILE)
}

pub fn load() -> (Config, Vec<String>) {
    match fs::read_to_string(path()) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), Vec::new()),
        Err(e) => (Config::default(), vec![format!("{FILE}: {e}")]),
    }
}

pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut cfg = Config::default();
    let mut errs = Vec::new();
    let table: Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map_or(0, |s| text[..s.start.min(text.len())].matches('\n').count() + 1);
            errs.push(format!("{FILE}: line {line}: {}", e.message().trim()));
            return (cfg, errs);
        }
    };
    for (section, v) in &table {
        let Some(t) = v.as_table() else {
            errs.push(format!("{FILE}: {section:?} must be a [section]"));
            continue;
        };
        match section.as_str() {
            "editor" => editor(t, &mut cfg.settings, &mut errs),
            "lsp" => lsp(t, &mut cfg.settings, &mut errs),
            "theme" => cfg.theme = theme_section(t, &mut errs),
            "keys" => cfg.keymap = Keymap::from_table(t, &mut errs),
            _ => errs.push(format!("{FILE}: unknown section [{section}]")),
        }
    }
    (cfg, errs)
}

fn int(t: &Table, sec: &str, key: &str, min: i64, max: i64, errs: &mut Vec<String>) -> Option<i64> {
    let v = t.get(key)?;
    match v.as_integer() {
        Some(n) if (min..=max).contains(&n) => Some(n),
        _ => {
            errs.push(format!("{FILE}: [{sec}] {key} must be a number from {min} to {max}"));
            None
        }
    }
}

fn boolean(t: &Table, sec: &str, key: &str, errs: &mut Vec<String>) -> Option<bool> {
    let v = t.get(key)?;
    if v.as_bool().is_none() {
        errs.push(format!("{FILE}: [{sec}] {key} must be true or false"));
    }
    v.as_bool()
}

fn unknown(t: &Table, sec: &str, known: &[&str], errs: &mut Vec<String>) {
    for k in t.keys().filter(|k| !known.contains(&k.as_str())) {
        errs.push(format!("{FILE}: [{sec}] unknown setting {k:?}"));
    }
}

fn editor(t: &Table, s: &mut Settings, errs: &mut Vec<String>) {
    unknown(t, "editor", &["tab_width", "autosave", "autosave_delay", "sidebar_width", "icons", "line_numbers"], errs);
    if let Some(n) = int(t, "editor", "tab_width", 1, 16, errs) {
        s.tab_width = n as usize;
    }
    if let Some(b) = boolean(t, "editor", "autosave", errs) {
        s.autosave = b;
    }
    if let Some(n) = int(t, "editor", "autosave_delay", 100, 600_000, errs) {
        s.autosave_delay = n as u64;
    }
    if let Some(n) = int(t, "editor", "sidebar_width", 10, 120, errs) {
        s.sidebar_width = n as u16;
    }
    if let Some(b) = boolean(t, "editor", "icons", errs) {
        s.icons = b;
    }
    if let Some(b) = boolean(t, "editor", "line_numbers", errs) {
        s.line_numbers = b;
    }
}

fn lsp(t: &Table, s: &mut Settings, errs: &mut Vec<String>) {
    unknown(t, "lsp", &["enabled", "idle_timeout"], errs);
    if let Some(b) = boolean(t, "lsp", "enabled", errs) {
        s.lsp_enabled = b;
    }
    if let Some(n) = int(t, "lsp", "idle_timeout", 1, 86_400, errs) {
        s.lsp_idle_timeout = n as u64;
    }
}

fn theme_section(t: &Table, errs: &mut Vec<String>) -> Theme {
    let mut th = theme::TOKYONIGHT;
    if let Some(v) = t.get("name") {
        match v.as_str().and_then(theme::builtin) {
            Some(b) => th = b,
            None => errs.push(format!("{FILE}: [theme] name must be one of {}", theme::NAMES.join(", "))),
        }
    }
    for (k, v) in t.iter().filter(|(k, _)| *k != "name") {
        let res = v
            .as_str()
            .ok_or_else(|| "must be a \"#rrggbb\" string".to_string())
            .and_then(theme::parse_color)
            .and_then(|c| th.set(k, c));
        if let Err(e) = res {
            errs.push(format!("{FILE}: [theme] {k}: {e}"));
        }
    }
    th
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

pub fn parse_key(s: &str) -> Result<Key, String> {
    let lower = s.trim().to_lowercase();
    let mut parts: Vec<&str> = lower.split('+').collect();
    if lower.ends_with("++") {
        parts.pop();
        parts.pop();
        parts.push("+");
    }
    let name = parts.pop().filter(|n| !n.is_empty()).ok_or_else(|| format!("{s:?}: missing key"))?;
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    for m in parts {
        match m {
            "ctrl" | "control" => ctrl = true,
            "alt" | "meta" | "option" => alt = true,
            "shift" => shift = true,
            _ => return Err(format!("{s:?}: unknown modifier {m:?} (use ctrl, alt, shift)")),
        }
    }
    let code = match name {
        "space" => KeyCode::Char(' '),
        "tab" if shift => KeyCode::BackTab,
        "tab" => KeyCode::Tab,
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "delete" | "del" => KeyCode::Delete,
        "backspace" => KeyCode::Backspace,
        f if f.starts_with('f') && f.len() > 1 && f[1..].chars().all(|c| c.is_ascii_digit()) => {
            let n: u8 = f[1..].parse().map_err(|_| format!("{s:?}: bad function key"))?;
            if !(1..=12).contains(&n) {
                return Err(format!("{s:?}: function keys are f1-f12"));
            }
            KeyCode::F(n)
        }
        c if c.chars().count() == 1 => KeyCode::Char(c.chars().next().unwrap()),
        _ => return Err(format!("{s:?}: unknown key {name:?}")),
    };
    if !ctrl && !alt && !matches!(code, KeyCode::F(_)) {
        return Err(format!("{s:?}: needs ctrl or alt (only f1-f12 work alone, other keys are for typing)"));
    }
    Ok(Key { code, ctrl, alt, shift })
}

impl Key {
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        let m = ev.modifiers;
        if m.contains(KeyModifiers::CONTROL) != self.ctrl || m.contains(KeyModifiers::ALT) != self.alt {
            return false;
        }
        match (self.code, ev.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => {
                a == b.to_ascii_lowercase() && (!self.shift || m.contains(KeyModifiers::SHIFT) || b.is_ascii_uppercase())
            }
            (a, b) => a == b && m.contains(KeyModifiers::SHIFT) == (self.shift && a != KeyCode::BackTab),
        }
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        let name = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::BackTab => "Tab".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            other => format!("{other:?}"),
        };
        s + &name
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    binds: Vec<(Key, Cmd)>,
}

impl Keymap {
    pub fn defaults() -> Keymap {
        let binds = COMMANDS
            .iter()
            .flat_map(|c| c.keys.iter().map(move |k| (parse_key(k).expect("valid default key"), c.cmd)))
            .collect();
        Keymap { binds }
    }

    fn from_table(t: &Table, errs: &mut Vec<String>) -> Keymap {
        let mut user: Vec<(Key, Cmd)> = Vec::new();
        let mut overridden: Vec<Cmd> = Vec::new();
        for (id, v) in t {
            let Some(c) = COMMANDS.iter().find(|c| c.id == id) else {
                errs.push(format!("{FILE}: unknown command {id:?} in [keys]"));
                continue;
            };
            let specs: Vec<&str> = match v {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) if a.iter().all(Value::is_str) => a.iter().filter_map(Value::as_str).collect(),
                _ => {
                    errs.push(format!("{FILE}: [keys] {id}: use \"ctrl+x\" or a list like [\"f12\", \"ctrl+g\"]"));
                    continue;
                }
            };
            let parsed: Result<Vec<Key>, String> =
                specs.iter().filter(|s| !s.eq_ignore_ascii_case("none") && !s.is_empty()).map(|s| parse_key(s)).collect();
            match parsed {
                Ok(keys) => {
                    overridden.push(c.cmd);
                    user.extend(keys.into_iter().map(|k| (k, c.cmd)));
                }
                Err(e) => errs.push(format!("{FILE}: [keys] {id}: {e}")),
            }
        }
        let mut binds = user.clone();
        for (k, c) in Keymap::defaults().binds {
            if !overridden.contains(&c) && !user.iter().any(|(uk, _)| *uk == k) {
                binds.push((k, c));
            }
        }
        Keymap { binds }
    }

    pub fn lookup(&self, ev: &KeyEvent) -> Option<Cmd> {
        self.binds.iter().find(|(k, _)| k.matches(ev)).map(|(_, c)| *c)
    }

    pub fn label(&self, cmd: Cmd) -> String {
        self.binds.iter().find(|(_, c)| *c == cmd).map(|(k, _)| k.label()).unwrap_or_default()
    }
}

pub fn template() -> String {
    let d = Settings::default();
    let mut keys = String::new();
    for c in COMMANDS {
        let v = match c.keys {
            [] => "\"none\"".to_string(),
            [one] => format!("{one:?}"),
            many => format!("[{}]", many.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(", ")),
        };
        keys.push_str(&format!("{:<16}= {v:<24}# {}\n", c.id, c.name));
    }
    format!(
        r##"# nib config — TOML syntax. Save this file inside nib and it applies instantly.
# Delete any line to go back to its default. Check it with: nib config check

[editor]
tab_width = {tab}          # spaces inserted by Tab, and tab display width (1-16)
autosave = {autosave}         # save automatically after you stop typing
autosave_delay = {delay}    # milliseconds after the last edit
sidebar_width = {side}     # file tree width in columns
icons = {icons}            # file icons (needs a Nerd Font)
line_numbers = {nums}

[keys]
# command = "key"  or  ["key1", "key2"];  "none" removes a shortcut.
# Keys: ctrl/alt/shift + a-z 0-9 punctuation f1-f12 space tab enter esc
#       up down left right home end pageup pagedown delete backspace.
# Plain keys (without ctrl/alt) are only allowed for f1-f12.
{keys}
[theme]
name = "tokyonight"         # {names}
# Override any color with "#rrggbb":
# text, dim, accent, bar, selection, warning, error,
# keyword, string, comment, number, function, type, tag, attribute
# keyword = "#bb9af7"

[lsp]
enabled = {lsp}             # language servers (see: nib plugin list)
idle_timeout = {idle}        # seconds before an unused server is stopped (plugins can override)
"##,
        tab = d.tab_width,
        autosave = d.autosave,
        delay = d.autosave_delay,
        side = d.sidebar_width,
        icons = d.icons,
        nums = d.line_numbers,
        names = theme::NAMES.join(" | "),
        lsp = d.lsp_enabled,
        idle = d.lsp_idle_timeout,
    )
}

pub fn cli(args: &[String]) -> i32 {
    let p = path();
    match args.first().map(String::as_str) {
        None | Some("init") => {
            if p.exists() {
                println!("{}", p.display());
                return 0;
            }
            match fs::create_dir_all(dir()).and_then(|_| fs::write(&p, template())) {
                Ok(()) => {
                    println!("Created {}", p.display());
                    0
                }
                Err(e) => {
                    eprintln!("Can't write {}: {e}", p.display());
                    1
                }
            }
        }
        Some("path") => {
            println!("{}", p.display());
            0
        }
        Some("check") => {
            let (_, errs) = load();
            if errs.is_empty() {
                println!("OK");
                0
            } else {
                for e in errs {
                    eprintln!("{e}");
                }
                1
            }
        }
        _ => {
            eprintln!("usage: nib config [init | path | check]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn template_is_valid_and_matches_defaults() {
        let (cfg, errs) = parse(&template());
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(cfg.settings, Settings::default());
        for c in COMMANDS {
            assert_eq!(cfg.keymap.label(c.cmd), Keymap::defaults().label(c.cmd), "{}", c.id);
        }
    }

    #[test]
    fn parses_keys() {
        let k = parse_key("Ctrl+Space").unwrap();
        assert_eq!((k.code, k.ctrl), (KeyCode::Char(' '), true));
        assert_eq!(parse_key("alt+left").unwrap().label(), "Alt+Left");
        assert_eq!(parse_key("f12").unwrap().label(), "F12");
        assert_eq!(parse_key("ctrl+shift+p").unwrap().label(), "Ctrl+Shift+P");
        assert_eq!(parse_key("ctrl++").unwrap().code, KeyCode::Char('+'));
        for bad in ["x", "enter", "hyper+x", "ctrl+f13", "ctrl+banana", ""] {
            assert!(parse_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn key_matching() {
        let ctrl = KeyModifiers::CONTROL;
        assert!(parse_key("ctrl+s").unwrap().matches(&ev(KeyCode::Char('s'), ctrl)));
        assert!(!parse_key("ctrl+s").unwrap().matches(&ev(KeyCode::Char('s'), KeyModifiers::NONE)));
        assert!(!parse_key("ctrl+s").unwrap().matches(&ev(KeyCode::Char('s'), ctrl | KeyModifiers::ALT)));
        assert!(parse_key("ctrl+shift+p").unwrap().matches(&ev(KeyCode::Char('P'), ctrl | KeyModifiers::SHIFT)));
        assert!(parse_key("alt+left").unwrap().matches(&ev(KeyCode::Left, KeyModifiers::ALT)));
        assert!(parse_key("f1").unwrap().matches(&ev(KeyCode::F(1), KeyModifiers::NONE)));
    }

    #[test]
    fn user_keys_replace_defaults_and_steal_keys() {
        let (cfg, errs) = parse("[keys]\nsave = \"ctrl+o\"\nquit = [\"ctrl+s\", \"f10\"]\nhover = \"none\"");
        assert!(errs.is_empty(), "{errs:?}");
        let km = cfg.keymap;
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(km.lookup(&ev(KeyCode::Char('o'), ctrl)), Some(Cmd::Save));
        assert_eq!(km.lookup(&ev(KeyCode::Char('s'), ctrl)), Some(Cmd::Quit));
        assert_eq!(km.lookup(&ev(KeyCode::Char('q'), ctrl)), None, "quit's default replaced");
        assert_eq!(km.lookup(&ev(KeyCode::F(1), KeyModifiers::NONE)), None, "unbound");
        assert_eq!(km.lookup(&ev(KeyCode::Char('p'), ctrl)), Some(Cmd::Palette), "others untouched");
        assert_eq!(km.label(Cmd::Quit), "Ctrl+S");
    }

    #[test]
    fn errors_are_reported_and_fall_back() {
        let (cfg, errs) = parse("[editor]\ntab_width = 99\nicons = \"yes\"\ncolour = 1\n[keys]\nsav = \"ctrl+o\"\nsave = \"x\"\n[theme]\nname = \"neon\"\nkeyword = \"red\"\n[nope]\n");
        let all = errs.join("\n");
        for want in ["tab_width", "icons", "unknown setting \"colour\"", "unknown command \"sav\"", "[keys] save", "[theme] name", "[theme] keyword", "unknown section [nope]"] {
            assert!(all.contains(want), "missing {want:?} in:\n{all}");
        }
        assert!(errs.iter().all(|e| e.starts_with("config.nib: ")));
        assert_eq!(cfg.settings.tab_width, 4);
        assert_eq!(cfg.keymap.label(Cmd::Save), "Ctrl+S", "bad binding keeps the default");

        let (_, errs) = parse("[editor]\ntab_width = 4\nautosave = tru\n");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("config.nib: line 3:"), "{errs:?}");
    }

    #[test]
    fn theme_and_settings() {
        let (cfg, errs) = parse("[editor]\ntab_width = 2\nsidebar_width = 20\n[theme]\nname = \"gruvbox\"\nstring = \"#010203\"\n[lsp]\nenabled = false\nidle_timeout = 5");
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!((cfg.settings.tab_width, cfg.settings.sidebar_width), (2, 20));
        assert_eq!(cfg.theme.keyword, theme::GRUVBOX.keyword);
        assert_eq!(cfg.theme.string, ratatui_core::style::Color::Rgb(1, 2, 3));
        assert!(!cfg.settings.lsp_enabled && cfg.settings.lsp_idle_timeout == 5);
    }
}
