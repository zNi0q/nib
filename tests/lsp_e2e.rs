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

fn fake_server() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_lsp.py").display().to_string()
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("nib-lsp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("proj")).unwrap();
        fs::create_dir_all(d.join("cfg/nib/plugins")).unwrap();
        Fixture(d)
    }
    fn cfg(&self) -> PathBuf {
        self.0.join("cfg")
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
    fn logfile(&self) -> PathBuf {
        self.0.join("server.log")
    }
    fn log(&self) -> String {
        fs::read_to_string(self.logfile()).unwrap_or_default()
    }
    fn wait_log(&self, method: &str) {
        let ok = wait_until(TIMEOUT, || self.log().lines().any(|l| l == method));
        assert!(ok, "server never received {method}; log:\n{}", self.log());
    }
    fn fake_plugin(&self, extra: &str) {
        let toml = format!(
            "name = \"fake\"\ncommand = \"python3\"\nargs = [{:?}]\nextensions = [\"fk\"]\n\
             language_ids = {{ fk = \"fake\" }}\nidle_timeout = 1\n\
             env = {{ FAKE_PIDFILE = {:?}, FAKE_LOG = {:?} }}\n{extra}",
            fake_server(),
            self.pidfile().display().to_string(),
            self.logfile().display().to_string(),
        );
        fs::write(self.cfg().join("nib/plugins/fake.toml"), toml).unwrap();
    }
    fn helper_pidfile(&self) -> PathBuf {
        self.0.join("server.pid.helper")
    }
    fn helper_pid(&self) -> u32 {
        let ok = wait_until(TIMEOUT, || fs::read_to_string(self.helper_pidfile()).is_ok_and(|s| s.trim().parse::<u32>().is_ok()));
        assert!(ok, "fake server never started its helper");
        fs::read_to_string(self.helper_pidfile()).unwrap().trim().parse().unwrap()
    }
    fn server_pid(&self) -> u32 {
        let ok = wait_until(TIMEOUT, || fs::read_to_string(self.pidfile()).is_ok_and(|s| s.trim().parse::<u32>().is_ok()));
        assert!(ok, "fake server never started (no pidfile)");
        fs::read_to_string(self.pidfile()).unwrap().trim().parse().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for f in [self.pidfile(), self.helper_pidfile()] {
            if let Ok(pid) = fs::read_to_string(f) {
                let _ = Command::new("kill").args(["-9", pid.trim()]).stderr(Stdio::null()).status();
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && !fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.contains(") Z "))
}

struct Tmux(String);

impl Tmux {
    fn start(test: &str, fx: &Fixture, file: &Path) -> Self {
        let name = format!("nib-lsp-{test}-{}", std::process::id());
        let _ = Command::new("tmux").args(["kill-session", "-t", &name]).stderr(Stdio::null()).status();
        let ok = Command::new("tmux")
            .args(["new-session", "-d", "-s", &name, "-x", "160", "-y", "40", "-c"])
            .arg(fx.0.join("proj"))
            .arg("env")
            .arg(format!("XDG_CONFIG_HOME={}", fx.cfg().display()))
            .arg(env!("CARGO_BIN_EXE_nib"))
            .arg(file)
            .status()
            .unwrap()
            .success();
        assert!(ok, "tmux failed to start");
        let t = Tmux(name);
        assert!(wait_until(TIMEOUT, || t.screen().contains("NIB")), "nib never rendered");
        t
    }
    fn keys(&self, keys: &[&str]) {
        Command::new("tmux").args(["send-keys", "-t", &self.0]).args(keys).status().unwrap();
        sleep(Duration::from_millis(80));
    }
    fn literal(&self, text: &str) {
        Command::new("tmux").args(["send-keys", "-t", &self.0, "-l", text]).status().unwrap();
        sleep(Duration::from_millis(80));
    }
    fn screen(&self) -> String {
        let out = Command::new("tmux").args(["capture-pane", "-p", "-t", &self.0]).output().unwrap();
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
    fn wait_gone(&self, text: &str) {
        let ok = wait_until(TIMEOUT, || !self.screen().contains(text));
        assert!(ok, "{text:?} still on screen:\n{}", self.screen());
    }
    fn wait_line(&self, what: &str, f: impl Fn(&str) -> bool) {
        let ok = wait_until(TIMEOUT, || self.screen().lines().any(&f));
        assert!(ok, "never saw a line with {what}:\n{}", self.screen());
    }
    fn wait_exit(&self) {
        let gone = || !Command::new("tmux").args(["has-session", "-t", &self.0]).stderr(Stdio::null()).status().unwrap().success();
        assert!(wait_until(TIMEOUT, gone), "nib did not exit");
    }
    fn wait_ready(&self) {
        let ok = wait_until(TIMEOUT, || self.status_bar().contains("fake"));
        assert!(ok, "server never became ready:\n{}", self.screen());
    }
    fn palette(&self, filter: &str) {
        self.keys(&["C-p"]);
        self.wait_for(" Commands ");
        self.literal(filter);
        self.keys(&["Enter"]);
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["kill-session", "-t", &self.0]).stderr(Stdio::null()).status();
    }
}

#[test]
fn diagnostics_show_in_gutter_status_and_message() {
    let fx = Fixture::new("diag");
    fx.fake_plugin("");
    let f = fx.write("a.fk", "hello\nlet x = BAD\nend\n");
    let t = Tmux::start("diag", &fx, &f);
    t.wait_ready();

    t.wait_line("● next to BAD", |l| l.contains("BAD") && l.contains('●'));
    let ok = wait_until(TIMEOUT, || t.status_bar().contains("✖ 1"));
    assert!(ok, "no error count in status bar:\n{}", t.screen());
    assert!(!t.line_with("hello").contains('●'), "clean line marked:\n{}", t.screen());
    fx.wait_log("textDocument/didOpen");

    t.keys(&["Down"]);
    t.wait_for("error: bad thing here");

    t.keys(&["Down", "End"]);
    t.literal(" WARN");
    t.wait_line("▲ next to WARN", |l| l.contains("end WARN") && l.contains('▲'));
    let ok = wait_until(TIMEOUT, || t.status_bar().contains("▲ 1"));
    assert!(ok, "no warning count in status bar:\n{}", t.screen());
    fx.wait_log("textDocument/didChange");
    fx.wait_disk("a.fk", "hello\nlet x = BAD\nend WARN\n");
    fx.wait_log("textDocument/didSave");

    t.keys(&["C-z"]);
    t.wait_gone("end WARN");
    let ok = wait_until(TIMEOUT, || !t.status_bar().contains("▲ 1") && !t.screen().lines().any(|l| l.contains('▲')));
    assert!(ok, "warning not cleared after undo:\n{}", t.screen());
    assert!(t.status_bar().contains("✖ 1"), "error count lost:\n{}", t.screen());
    fx.wait_disk("a.fk", "hello\nlet x = BAD\nend\n");
}

#[test]
fn hover_popup_opens_and_closes() {
    let fx = Fixture::new("hover");
    fx.fake_plugin("");
    let f = fx.write("h.fk", "hello world\n");
    let t = Tmux::start("hover", &fx, &f);
    t.wait_ready();

    t.keys(&["F1"]);
    t.wait_for(" Info ");
    t.wait_for("fake hover");
    t.wait_for("for hello");
    fx.wait_log("textDocument/hover");

    t.keys(&["Escape"]);
    t.wait_gone(" Info ");
    sleep(Duration::from_millis(1500));
    assert_eq!(fx.read("h.fk"), "hello world\n", "hover/Esc modified the file");
}

#[test]
fn go_to_definition_back_and_cross_file() {
    let fx = Fixture::new("def");
    fx.fake_plugin("");
    fx.write("other.fk", "zero\none two\n");
    let f = fx.write("d.fk", "target()\n\n\n\nfn target\nother\n");
    let t = Tmux::start("def", &fx, &f);
    t.wait_ready();
    t.wait_for("Ln 1, Col 1");

    t.keys(&["F12"]);
    t.wait_for("Ln 5, Col 1");
    fx.wait_log("textDocument/definition");

    t.keys(&["M-Left"]);
    t.wait_for("Ln 1, Col 1");

    t.keys(&["C-End", "Home"]);
    t.wait_for("Ln 6, Col 1");
    t.keys(&["F12"]);
    t.wait_for("one two");
    let ok = wait_until(TIMEOUT, || t.status_bar().contains("other.fk") && t.status_bar().contains("Ln 2, Col 4"));
    assert!(ok, "didn't land in other.fk at 2:4:\n{}", t.screen());
    assert_eq!(fx.read("d.fk"), "target()\n\n\n\nfn target\nother\n");
    assert_eq!(fx.read("other.fk"), "zero\none two\n");
}

#[test]
fn completion_popup_filters_inserts_and_auto_triggers() {
    let fx = Fixture::new("comp");
    fx.fake_plugin("");
    let f = fx.write("c.fk", "\n");
    let t = Tmux::start("comp", &fx, &f);
    t.wait_ready();

    t.keys(&["C-Space"]);
    t.wait_for(" Complete ");
    t.wait_for("alphabet");
    t.wait_for("beta");
    t.wait_for("first");
    fx.wait_log("textDocument/completion");

    t.literal("alphab");
    t.wait_gone("beta");
    t.wait_gone("first");
    t.wait_for("alphabet");
    t.keys(&["Enter"]);
    t.wait_gone(" Complete ");
    fx.wait_disk("c.fk", "alphabet\n");

    t.literal(" x.");
    t.wait_for(" Complete ");
    t.wait_for("beta");
    t.keys(&["Escape"]);
    t.wait_gone(" Complete ");
    fx.wait_disk("c.fk", "alphabet x.\n");
}

#[test]
fn idle_server_is_stopped_after_file_closes() {
    let fx = Fixture::new("idle");
    fx.fake_plugin("");
    let f = fx.write("i.fk", "idle\n");
    let t = Tmux::start("idle", &fx, &f);
    t.wait_ready();
    let pid = fx.server_pid();
    assert!(alive(pid), "server {pid} not running");

    t.palette("lsp status");
    t.wait_for(&format!("LSP: fake (pid {pid},"));
    t.wait_for(" MB)");

    let helper = fx.helper_pid();
    t.keys(&["C-w"]);
    let ok = wait_until(Duration::from_secs(6), || !alive(pid) && !alive(helper));
    assert!(ok, "server {pid} or its helper {helper} still running after the file closed");
    fx.wait_log("textDocument/didClose");
    fx.wait_log("shutdown");

    t.palette("lsp status");
    t.wait_for("LSP: no servers running");
}

#[test]
fn missing_server_shows_install_hint_and_editing_still_works() {
    let fx = Fixture::new("missing");
    fs::write(
        fx.cfg().join("nib/plugins/nope.toml"),
        "name = \"nope\"\ncommand = \"definitely-not-a-real-lsp\"\nextensions = [\"fk\"]\ninstall = \"npm i -g x\"\n",
    )
    .unwrap();
    let f = fx.write("m.fk", "abc\n");
    let t = Tmux::start("missing", &fx, &f);
    t.wait_for("definitely-not-a-real-lsp not found — install: npm i -g x");

    t.literal("hi");
    t.wait_for("hiabc");
    fx.wait_disk("m.fk", "hiabc\n");
}

fn nib_cli(fx: &Fixture, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_nib"))
        .env("XDG_CONFIG_HOME", fx.cfg())
        .args(args)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn plugin_cli_add_list_remove_new() {
    let fx = Fixture::new("cli");
    let plugins = fx.cfg().join("nib/plugins");
    let ts = plugins.join("typescript.toml");

    let (ok, out) = nib_cli(&fx, &["plugin", "presets"]);
    assert!(ok, "{out}");
    for p in ["typescript", "vue", "svelte", "html", "css", "json", "python", "sql", "rust", "go"] {
        assert!(out.contains(p), "preset {p} missing:\n{out}");
    }

    let (ok, out) = nib_cli(&fx, &["plugin", "add", "typescript"]);
    assert!(ok, "{out}");
    assert!(out.contains("Added typescript"), "{out}");
    assert!(fs::read_to_string(&ts).unwrap().contains("--lsp"), "typescript preset uses the native TS 7 server");

    let (ok, out) = nib_cli(&fx, &["plugin", "add", "typescript"]);
    assert!(!ok, "second add should fail:\n{out}");
    assert!(out.contains("already exists"), "{out}");

    let (ok, out) = nib_cli(&fx, &["plugin", "list"]);
    assert!(ok, "{out}");
    let line = out.lines().find(|l| l.starts_with("typescript")).unwrap_or_else(|| panic!("not listed:\n{out}"));
    assert!(line.contains(" tsc "), "{line}");
    assert!(line.contains("NOT FOUND") || line.contains("(/"), "no resolution shown: {line}");
    assert!(out.contains("Presets:"), "{out}");

    let (ok, out) = nib_cli(&fx, &["plugin", "remove", "typescript"]);
    assert!(ok && out.contains("Removed"), "{out}");
    assert!(!ts.exists());

    let (ok, out) = nib_cli(&fx, &["plugin", "new", "mylang"]);
    assert!(ok, "{out}");
    assert!(plugins.join("mylang.toml").exists(), "{out}");
}

#[test]
fn quitting_stops_servers_and_their_helpers() {
    let fx = Fixture::new("quit");
    fx.fake_plugin("");
    let f = fx.write("q.fk", "quit\n");
    let t = Tmux::start("quit", &fx, &f);
    t.wait_ready();
    let (pid, helper) = (fx.server_pid(), fx.helper_pid());
    assert!(alive(pid) && alive(helper));
    t.keys(&["C-q"]);
    t.wait_exit();
    let ok = wait_until(Duration::from_secs(2), || !alive(pid) && !alive(helper));
    assert!(ok, "left running after quit: server {pid} alive={} helper {helper} alive={}", alive(pid), alive(helper));
    fx.wait_log("shutdown");
}

#[test]
fn stubborn_server_is_force_stopped_on_quit() {
    let fx = Fixture::new("stubborn");
    fx.fake_plugin("");
    fs::write(fx.0.join("server.pid.stubborn"), "").unwrap();
    let f = fx.write("s.fk", "stubborn\n");
    let t = Tmux::start("stubborn", &fx, &f);
    t.wait_ready();
    let (pid, helper) = (fx.server_pid(), fx.helper_pid());
    let start = std::time::Instant::now();
    t.keys(&["C-q"]);
    t.wait_exit();
    assert!(start.elapsed() < Duration::from_secs(3), "quit took {:?}", start.elapsed());
    let ok = wait_until(Duration::from_secs(2), || !alive(pid) && !alive(helper));
    assert!(ok, "stubborn server {pid} or helper {helper} survived quit");
}

#[test]
fn closing_the_terminal_saves_and_cleans_up() {
    let fx = Fixture::new("hangup");
    fx.fake_plugin("");
    let f = fx.write("h.fk", "before\n");
    let t = Tmux::start("hangup", &fx, &f);
    t.wait_ready();
    let (pid, helper) = (fx.server_pid(), fx.helper_pid());
    t.literal("typed ");
    t.wait_for("typed before");
    let _ = Command::new("tmux").args(["kill-session", "-t", &t.0]).status();
    fx.wait_disk("h.fk", "typed before\n");
    let ok = wait_until(Duration::from_secs(3), || !alive(pid) && !alive(helper));
    assert!(ok, "left running after the terminal closed: server {pid} helper {helper}");
    let nib_running = || !Command::new("pgrep").args(["-f", &f.display().to_string()]).output().unwrap().stdout.is_empty();
    assert!(wait_until(Duration::from_secs(5), || !nib_running()), "nib still running 5 s after the terminal closed");
}
