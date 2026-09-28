//! End-to-end tests for the config system: run the real `nib` binary in tmux
//! with a temp XDG_CONFIG_HOME containing `nib/config.nib`, and check both
//! the screen (including colors via `capture-pane -e`) and files on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(8);

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    false
}

/// Temp dir with `cfg/` (XDG_CONFIG_HOME) and `proj/` (files to edit).
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("nib-cfg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("proj")).unwrap();
        fs::create_dir_all(d.join("cfg/nib/plugins")).unwrap();
        Fixture(d)
    }
    fn cfg(&self) -> PathBuf {
        self.0.join("cfg")
    }
    fn config_file(&self) -> PathBuf {
        self.cfg().join("nib/config.nib")
    }
    fn config(&self, text: &str) {
        fs::write(self.config_file(), text).unwrap();
    }
    fn proj(&self, rel: &str) -> PathBuf {
        self.0.join("proj").join(rel)
    }
    fn write(&self, rel: &str, data: &str) -> PathBuf {
        let p = self.proj(rel);
        fs::write(&p, data).unwrap();
        p
    }
    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.proj(rel)).unwrap_or_default()
    }
    fn wait_disk(&self, rel: &str, want: &str) {
        let ok = wait_until(TIMEOUT, || self.read(rel) == want);
        assert!(ok, "{rel} on disk: {:?}, want {:?}", self.read(rel), want);
    }
    fn pidfile(&self) -> PathBuf {
        self.0.join("server.pid")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(self.pidfile()) {
            let _ = Command::new("kill").arg(pid.trim()).stderr(Stdio::null()).status();
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Tmux(String);

impl Tmux {
    fn start(test: &str, fx: &Fixture, arg: &Path) -> Self {
        let name = format!("nib-cfg-{test}-{}", std::process::id());
        let _ = Command::new("tmux").args(["kill-session", "-t", &name]).stderr(Stdio::null()).status();
        let ok = Command::new("tmux")
            .args(["new-session", "-d", "-s", &name, "-x", "160", "-y", "40", "-c"])
            .arg(fx.0.join("proj"))
            .arg("env")
            .arg(format!("XDG_CONFIG_HOME={}", fx.cfg().display()))
            .arg(env!("CARGO_BIN_EXE_nib"))
            .arg(arg)
            .status()
            .unwrap()
            .success();
        assert!(ok, "tmux failed to start");
        let t = Tmux(name);
        assert!(wait_until(TIMEOUT, || t.screen().contains("NIB")), "nib never rendered:\n{}", t.screen());
        t
    }
    fn keys(&self, keys: &[&str]) {
        Command::new("tmux").args(["send-keys", "-t", &self.0]).args(keys).status().unwrap();
        sleep(Duration::from_millis(100));
    }
    fn literal(&self, text: &str) {
        Command::new("tmux").args(["send-keys", "-t", &self.0, "-l", text]).status().unwrap();
        sleep(Duration::from_millis(100));
    }
    fn screen(&self) -> String {
        let out = Command::new("tmux").args(["capture-pane", "-p", "-t", &self.0]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    /// Screen with color escape sequences.
    fn screen_colors(&self) -> String {
        let out = Command::new("tmux").args(["capture-pane", "-e", "-p", "-t", &self.0]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    fn status_bar(&self) -> String {
        self.screen().lines().find(|l| l.contains("Ln ")).unwrap_or_default().to_string()
    }
    fn line_with(&self, text: &str) -> String {
        self.screen().lines().find(|l| l.contains(text)).unwrap_or_default().to_string()
    }
    fn wait_for(&self, text: &str) {
        let ok = wait_until(TIMEOUT, || self.screen().contains(text));
        assert!(ok, "never saw {text:?} on screen:\n{}", self.screen());
    }
    fn wait_colors(&self, esc: &str) {
        let ok = wait_until(TIMEOUT, || self.screen_colors().contains(esc));
        assert!(ok, "never saw color {esc:?}:\n{}", self.screen_colors());
    }
    fn alive(&self) -> bool {
        Command::new("tmux").args(["has-session", "-t", &self.0]).stderr(Stdio::null()).status().unwrap().success()
    }
    fn wait_exit(&self) {
        assert!(wait_until(TIMEOUT, || !self.alive()), "nib did not exit:\n{}", self.screen());
    }
    fn no_config_error(&self) {
        sleep(Duration::from_millis(300));
        assert!(!self.screen().contains("config.nib:"), "unexpected config error:\n{}", self.screen());
    }
    /// Open the palette, filter, and return the screen line containing `row`.
    fn palette_row(&self, filter: &str, row: &str) -> String {
        self.keys(&["C-p"]);
        self.wait_for(" Commands ");
        self.literal(filter);
        self.wait_for(row);
        let line = self.line_with(row);
        self.keys(&["Escape"]);
        line
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["kill-session", "-t", &self.0]).stderr(Stdio::null()).status();
    }
}

fn nib_cli(fx: &Fixture, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_nib")).env("XDG_CONFIG_HOME", fx.cfg()).args(args).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

// ---------- 1. custom keybinding ----------

#[test]
fn rebinding_save_replaces_the_default_key() {
    let fx = Fixture::new("rebind");
    fx.config("[editor]\nautosave = false\n\n[keys]\nsave = \"ctrl+o\"\n");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("rebind", &fx, &f);
    t.no_config_error();

    t.literal("X");
    t.wait_for("Xalpha");
    t.keys(&["C-s"]);
    sleep(Duration::from_millis(500));
    assert_eq!(fx.read("a.txt"), "alpha\n", "Ctrl+S still saves after rebinding save to Ctrl+O");
    assert!(!t.screen().contains("Saved a.txt"), "{}", t.screen());

    t.keys(&["C-o"]);
    t.wait_for("Saved a.txt");
    fx.wait_disk("a.txt", "Xalpha\n");

    let row = t.palette_row("save f", "Save file");
    assert!(row.contains("Ctrl+O"), "palette row: {row:?}");
    assert!(!row.contains("Ctrl+S"), "palette row: {row:?}");
}

// ---------- 2. several keys for one command ----------

#[test]
fn list_of_keys_is_accepted_and_first_is_shown() {
    let fx = Fixture::new("multi");
    fx.config("[keys]\ndefinition = [\"f12\", \"ctrl+g\"]\n");
    let f = fx.write("a.txt", "x\n");
    let t = Tmux::start("multi", &fx, &f);
    t.no_config_error();
    let row = t.palette_row("go to def", "Go to definition");
    assert!(row.contains("F12"), "palette row: {row:?}");
}

// ---------- 3. editor settings ----------

#[test]
fn tab_width_sets_spaces_per_tab() {
    let fx = Fixture::new("tab");
    fx.config("[editor]\ntab_width = 2\n");
    let f = fx.write("t.txt", "x\n");
    let t = Tmux::start("tab", &fx, &f);
    t.no_config_error();
    t.keys(&["Tab"]);
    t.wait_for("Ln 1, Col 3");
    t.keys(&["C-s"]);
    fx.wait_disk("t.txt", "  x\n");
}

#[test]
fn sidebar_width_moves_the_tree_border() {
    let fx = Fixture::new("sidebar");
    fx.config("[editor]\nsidebar_width = 20\n");
    fx.write("a.txt", "x\n");
    fx.write("b.txt", "y\n");
    let t = Tmux::start("sidebar", &fx, &fx.0.join("proj"));
    t.wait_for("b.txt");
    t.no_config_error();
    let screen = t.screen();
    let rows: Vec<&str> = screen.lines().skip(1).take(5).collect();
    for r in &rows {
        assert_eq!(r.chars().nth(19), Some('│'), "border not at column 19 in {r:?}\n{screen}");
    }
}

#[test]
fn line_numbers_can_be_hidden() {
    let fx = Fixture::new("nonum");
    fx.config("[editor]\nline_numbers = false\n");
    let f = fx.write("n.txt", "hello\nworld\n");
    let t = Tmux::start("nonum", &fx, &f);
    t.wait_for("world");
    t.no_config_error();
    let l = t.line_with("hello");
    let after_border = l.split('│').nth(1).unwrap_or(&l);
    assert!(after_border.trim_start().starts_with("hello"), "line number still shown: {l:?}");
    assert!(!t.line_with("world").contains('2'), "{}", t.screen());
}

#[test]
fn autosave_off_brings_back_the_quit_prompt() {
    let fx = Fixture::new("noauto");
    fx.config("[editor]\nautosave = false\n");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("noauto", &fx, &f);
    t.no_config_error();
    t.literal("zz");
    t.wait_for("zzalpha");
    sleep(Duration::from_millis(1500));
    assert_eq!(fx.read("a.txt"), "alpha\n", "auto-saved although autosave = false");
    t.keys(&["C-q"]);
    t.wait_for("Save changes to");
    t.keys(&["n"]);
    t.wait_exit();
    assert_eq!(fx.read("a.txt"), "alpha\n");
}

// ---------- 4. themes ----------

#[test]
fn builtin_themes_and_color_overrides() {
    let rust = "fn main() {\n    let x = 1;\n}\n";

    let fx = Fixture::new("gruvbox");
    fx.config("[theme]\nname = \"gruvbox\"\n");
    let f = fx.write("m.rs", rust);
    let t = Tmux::start("gruvbox", &fx, &f);
    t.wait_for("let x");
    t.no_config_error();
    t.wait_colors("38;2;251;73;52"); // gruvbox keyword #fb4934
    drop(t);

    let fx = Fixture::new("catppuccin");
    fx.config("[theme]\nname = \"catppuccin\"\n");
    let f = fx.write("m.rs", rust);
    let t = Tmux::start("catppuccin", &fx, &f);
    t.wait_for("let x");
    t.no_config_error();
    drop(t);

    let fx = Fixture::new("override");
    fx.config("[theme]\nkeyword = \"#010203\"\n");
    let f = fx.write("m.rs", rust);
    let t = Tmux::start("override", &fx, &f);
    t.wait_for("let x");
    t.no_config_error();
    t.wait_colors("38;2;1;2;3");
}

// ---------- 5. errors never break nib ----------

#[test]
fn invalid_toml_reports_line_and_editing_still_works() {
    let fx = Fixture::new("badtoml");
    fx.config("[editor]\ntab_width = 4\n[keys\nsave = \"ctrl+o\"\n");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("badtoml", &fx, &f);
    t.wait_for("config.nib:");
    assert!(t.line_with("config.nib:").contains("line"), "{}", t.screen());
    t.literal("ok ");
    t.wait_for("ok alpha");
    t.keys(&["C-s"]); // defaults apply: Ctrl+S saves
    fx.wait_disk("a.txt", "ok alpha\n");
}

#[test]
fn unknown_command_is_named_in_the_error() {
    let fx = Fixture::new("unknown");
    fx.config("[keys]\nsav = \"ctrl+o\"\n");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("unknown", &fx, &f);
    t.wait_for("config.nib:");
    assert!(t.line_with("config.nib:").contains("sav"), "{}", t.screen());
}

#[test]
fn plain_letter_binding_is_rejected_and_letter_still_types() {
    let fx = Fixture::new("letter");
    fx.config("[keys]\nsave = \"x\"\n");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("letter", &fx, &f);
    t.wait_for("config.nib:");
    t.literal("x");
    t.wait_for("xalpha");
    fx.wait_disk("a.txt", "xalpha\n"); // auto-save still on
}

#[test]
fn bad_color_is_an_error() {
    let fx = Fixture::new("badcolor");
    fx.config("[theme]\nkeyword = \"#zzzzzz\"\n");
    let f = fx.write("m.rs", "fn main() {}\n");
    let t = Tmux::start("badcolor", &fx, &f);
    t.wait_for("config.nib:");
    t.wait_for("fn main");
}

// ---------- 6. live reload ----------

#[test]
fn saving_config_inside_nib_reloads_it() {
    let fx = Fixture::new("reload");
    fx.config("[editor]\nautosave = false\n");
    let cfg = fx.config_file();
    let t = Tmux::start("reload", &fx, &cfg);
    t.wait_for("autosave = false");
    let ok = wait_until(TIMEOUT, || t.status_bar().contains("Config"));
    assert!(ok, ".nib not highlighted as Config:\n{}", t.status_bar());

    // Add a [keys] section and save with the (still default) Ctrl+S.
    t.keys(&["C-End"]);
    t.keys(&["Enter"]);
    t.literal("[keys]");
    t.keys(&["Enter"]);
    t.literal("save = \"ctrl+o\"");
    t.keys(&["C-s"]);
    t.wait_for("Config reloaded");
    let on_disk = fs::read_to_string(&cfg).unwrap();
    assert!(on_disk.contains("save = \"ctrl+o\""), "{on_disk:?}");

    // New binding is live: Ctrl+S no longer saves, Ctrl+O does.
    t.keys(&["Enter"]);
    t.literal("# after");
    t.keys(&["C-s"]);
    sleep(Duration::from_millis(500));
    assert!(!fs::read_to_string(&cfg).unwrap().contains("# after"), "Ctrl+S still saves after reload");
    t.keys(&["C-o"]);
    let ok = wait_until(TIMEOUT, || fs::read_to_string(&cfg).unwrap().contains("# after"));
    assert!(ok, "Ctrl+O did not save after reload:\n{}", t.screen());
    t.wait_for("Config reloaded");
}

// ---------- 7. CLI ----------

#[test]
fn config_cli_create_check_path() {
    let fx = Fixture::new("cli");
    let cfg = fx.config_file();
    let _ = fs::remove_file(&cfg);

    let (ok, out) = nib_cli(&fx, &["config", "path"]);
    assert!(ok, "{out}");
    assert_eq!(out.trim(), cfg.display().to_string());

    let (ok, out) = nib_cli(&fx, &["config"]);
    assert!(ok, "{out}");
    assert!(out.contains("Created"), "{out}");
    assert!(cfg.exists());
    let generated = fs::read_to_string(&cfg).unwrap();
    for section in ["[editor]", "[keys]", "[theme]", "[lsp]"] {
        assert!(generated.contains(section), "default config lacks {section}");
    }
    assert!(generated.contains('#'), "default config has no comments");

    let (ok, out) = nib_cli(&fx, &["config"]);
    assert!(ok, "{out}");
    assert!(!out.contains("Created") && out.contains(&cfg.display().to_string()), "{out}");
    assert_eq!(fs::read_to_string(&cfg).unwrap(), generated, "existing config overwritten");

    let (ok, out) = nib_cli(&fx, &["config", "check"]);
    assert!(ok, "generated config fails check: {out}");
    assert!(out.contains("OK"), "{out}");

    fs::write(&cfg, "[editor]\ntab_width = 4\n[keys\n").unwrap();
    let (ok, out) = nib_cli(&fx, &["config", "check"]);
    assert!(!ok, "broken config passed check: {out}");
    assert!(out.contains("line"), "{out}");
}

// ---------- 8. LSP can be switched off ----------

#[test]
fn lsp_disabled_starts_no_server() {
    let fx = Fixture::new("nolsp");
    let server = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_lsp.py");
    fs::write(
        fx.cfg().join("nib/plugins/fake.toml"),
        format!(
            "name = \"fake\"\ncommand = \"python3\"\nargs = [{:?}]\nextensions = [\"fk\"]\nenv = {{ FAKE_PIDFILE = {:?} }}\n",
            server.display().to_string(),
            fx.pidfile().display().to_string(),
        ),
    )
    .unwrap();
    fx.config("[lsp]\nenabled = false\n");
    let f = fx.write("a.fk", "hello BAD\n");
    let t = Tmux::start("nolsp", &fx, &f);
    t.wait_for("hello BAD");
    t.no_config_error();
    sleep(Duration::from_millis(1500));
    assert!(!fx.pidfile().exists(), "server started although [lsp] enabled = false");
    t.keys(&["C-p"]);
    t.wait_for(" Commands ");
    t.literal("lsp: st");
    t.keys(&["Enter"]);
    t.wait_for("no servers running");
    assert!(!t.status_bar().contains("fake"), "{}", t.status_bar());
}
