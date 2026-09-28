//! End-to-end tests: run the real `nib` binary inside tmux, drive it with
//! keystrokes and bracketed paste, and check both the screen and the files
//! on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

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

/// Fresh temp project dir for one test; removed on drop.
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("nib-e2e-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Fixture(d)
    }
    fn write(&self, rel: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, data).unwrap();
        p
    }
    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).unwrap_or_default()
    }
    /// Wait until `rel` on disk equals `want`.
    fn wait_disk(&self, rel: &str, want: &str) {
        let ok = wait_until(Duration::from_secs(5), || self.read(rel) == want);
        assert!(ok, "{rel} on disk: {:?}, want {:?}", self.read(rel), want);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Tmux(String);

impl Tmux {
    fn start(test: &str, cwd: &Path, arg: &Path) -> Self {
        let name = format!("nib-{test}-{}", std::process::id());
        let _ = Command::new("tmux").args(["kill-session", "-t", &name]).stderr(Stdio::null()).status();
        let ok = Command::new("tmux")
            .args(["new-session", "-d", "-s", &name, "-x", "160", "-y", "40", "-c"])
            .arg(cwd)
            .arg(env!("CARGO_BIN_EXE_nib"))
            .arg(arg)
            .status()
            .unwrap()
            .success();
        assert!(ok, "tmux failed to start");
        let t = Tmux(name);
        // Wait for the first frame (the status bar always shows "NIB").
        assert!(wait_until(Duration::from_secs(5), || t.screen().contains("NIB")), "nib never rendered");
        t
    }
    fn keys(&self, keys: &[&str]) {
        Command::new("tmux").args(["send-keys", "-t", &self.0]).args(keys).status().unwrap();
        sleep(Duration::from_millis(60));
    }
    fn literal(&self, text: &str) {
        Command::new("tmux").args(["send-keys", "-t", &self.0, "-l", text]).status().unwrap();
        sleep(Duration::from_millis(60));
    }
    /// Bracketed paste (`paste-buffer -p`), like pasting in a real terminal.
    fn paste(&self, text: &str) {
        let buf = format!("{}-buf", self.0);
        let mut child = Command::new("tmux")
            .args(["load-buffer", "-b", &buf, "-"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        Command::new("tmux").args(["paste-buffer", "-p", "-d", "-b", &buf, "-t", &self.0]).status().unwrap();
        sleep(Duration::from_millis(60));
    }
    fn screen(&self) -> String {
        let out = Command::new("tmux").args(["capture-pane", "-p", "-t", &self.0]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    fn wait_for(&self, text: &str) {
        let ok = wait_until(Duration::from_secs(5), || self.screen().contains(text));
        assert!(ok, "never saw {text:?} on screen:\n{}", self.screen());
    }
    fn alive(&self) -> bool {
        Command::new("tmux").args(["has-session", "-t", &self.0]).stderr(Stdio::null()).status().unwrap().success()
    }
    fn wait_exit(&self) {
        assert!(wait_until(Duration::from_secs(5), || !self.alive()), "nib didn't exit:\n{}", self.screen());
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["kill-session", "-t", &self.0]).stderr(Stdio::null()).status();
    }
}

#[test]
fn open_folder_edit_save_then_quit_autosaves() {
    let fx = Fixture::new("folder");
    fx.write("src/main.rs", "fn main() {}\n");
    fx.write("README.md", "# readme\n");
    fx.write(".git/config", "[core]\n");

    let t = Tmux::start("folder", &fx.0, &fx.0);
    t.wait_for("src/");
    t.wait_for("README.md");
    assert!(!t.screen().contains(".git"), ".git should be hidden:\n{}", t.screen());

    // src/ is first (folders first). Expand it, open main.rs.
    t.keys(&["Enter"]);
    t.wait_for("main.rs");
    t.keys(&["Down", "Enter"]);
    t.wait_for("fn main() {}");

    t.literal("// ");
    t.wait_for("●");
    t.keys(&["C-s"]);
    t.wait_for("Saved src/main.rs");
    fx.wait_disk("src/main.rs", "// fn main() {}\n");
    assert!(!t.screen().contains("●"), "still marked modified after save:\n{}", t.screen());

    // Quitting right after an edit saves it (auto-save), no prompt.
    t.literal("more");
    t.keys(&["C-q"]);
    t.wait_exit();
    assert_eq!(fx.read("src/main.rs"), "// morefn main() {}\n");
}

#[test]
fn autosaves_after_a_pause_without_ctrl_s() {
    let fx = Fixture::new("autosave");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("autosave", &fx.0, &f);
    t.wait_for("alpha");
    t.literal("typed ");
    t.wait_for("●");
    fx.wait_disk("a.txt", "typed alpha\n");
    t.wait_for("Saved a.txt");
    assert!(!t.screen().contains("●"), "still marked modified:\n{}", t.screen());
}

#[test]
fn autosave_off_asks_before_quitting_and_n_discards() {
    let fx = Fixture::new("noauto");
    let f = fx.write("a.txt", "alpha\n");
    let t = Tmux::start("noauto", &fx.0, &f);
    t.wait_for("alpha");
    t.keys(&["C-p"]);
    t.wait_for(" Commands ");
    t.literal("auto");
    t.keys(&["Enter"]);
    t.wait_for("Auto-save off");
    t.literal("zz");
    t.wait_for("zzalpha");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(fx.read("a.txt"), "alpha\n", "saved while auto-save was off");
    t.keys(&["C-q"]);
    t.wait_for("Save changes to");
    t.keys(&["n"]);
    t.wait_exit();
    assert_eq!(fx.read("a.txt"), "alpha\n", "discarded edit reached disk");
}

#[test]
fn switching_files_autosaves_without_prompt() {
    let fx = Fixture::new("switch");
    let a = fx.write("a.txt", "alpha\n");
    fx.write("b.txt", "beta\n");

    let t = Tmux::start("switch", &fx.0, &a);
    t.wait_for("alpha");
    t.literal("1");
    t.wait_for("1alpha");

    // Back to the tree (a.txt is selected), move to b.txt, open it.
    t.keys(&["Escape"]);
    t.keys(&["Down", "Enter"]);
    fx.wait_disk("a.txt", "1alpha\n");
    assert!(!t.screen().contains("Save changes to"), "prompted despite auto-save:\n{}", t.screen());
    t.wait_for("beta");
    assert!(!t.screen().contains("1alpha"), "old file still shown:\n{}", t.screen());
    fx.wait_disk("b.txt", "beta\n");
}

#[test]
fn new_file_bracketed_paste_is_verbatim() {
    let fx = Fixture::new("paste");
    let p = fx.0.join("newfile.py");
    assert!(!p.exists());

    let t = Tmux::start("paste", &fx.0, &p);
    t.wait_for("newfile.py");
    let code = "def f():\n    if x:\n        return 1\n    return 2";
    t.paste(code);
    t.wait_for("        return 1");
    t.wait_for("Ln 4, Col 13");
    t.keys(&["C-s"]);
    t.wait_for("Saved newfile.py");
    // New files end with a newline; the pasted indentation must be unchanged.
    fx.wait_disk("newfile.py", &format!("{code}\n"));
    t.wait_for("newfile.py");
}

#[test]
fn find_undo_redo_reach_disk() {
    let fx = Fixture::new("find");
    let f = fx.write("find.txt", "one two\nthree two\n");

    let t = Tmux::start("find", &fx.0, &f);
    t.wait_for("three two");
    t.wait_for("Ln 1, Col 1");

    t.keys(&["C-f"]);
    t.literal("two");
    t.keys(&["Enter"]);
    t.wait_for("Ln 1, Col 5");
    t.keys(&["Enter"]);
    t.wait_for("Ln 2, Col 7");
    t.keys(&["Escape"]);

    t.literal("X");
    t.wait_for("three Xtwo");
    t.keys(&["C-s"]);
    fx.wait_disk("find.txt", "one two\nthree Xtwo\n");

    t.keys(&["C-z"]);
    t.keys(&["C-s"]);
    fx.wait_disk("find.txt", "one two\nthree two\n");

    t.keys(&["C-y"]);
    t.keys(&["C-s"]);
    fx.wait_disk("find.txt", "one two\nthree Xtwo\n");
    t.wait_for("three Xtwo");
}

#[test]
fn binary_file_is_refused() {
    let fx = Fixture::new("binary");
    let bytes = [0x89u8, b'P', b'N', b'G', 0, 0, 1, 2];
    fx.write("img.bin", bytes);
    fx.write("ok.txt", "fine\n");

    let t = Tmux::start("binary", &fx.0, &fx.0);
    t.wait_for("img.bin");
    t.keys(&["Enter"]); // img.bin is first
    t.wait_for("Can't open img.bin: binary file");
    assert!(!t.screen().contains("PNG"), "binary contents shown:\n{}", t.screen());

    // No buffer opened, so quitting doesn't ask anything.
    t.keys(&["C-q"]);
    t.wait_exit();
    assert_eq!(fs::read(fx.0.join("img.bin")).unwrap(), bytes, "binary file modified");
}

#[test]
fn crlf_file_keeps_line_endings() {
    let fx = Fixture::new("crlf");
    let f = fx.write("crlf.txt", "a\r\nb\r\n");

    let t = Tmux::start("crlf", &fx.0, &f);
    t.wait_for("Ln 1, Col 1");
    t.literal("Z");
    t.wait_for("Za");
    t.keys(&["Down", "End"]);
    t.literal("!");
    t.keys(&["C-s"]);
    t.wait_for("Saved crlf.txt");
    fx.wait_disk("crlf.txt", "Za\r\nb!\r\n");
}

#[test]
fn command_palette_saves() {
    let fx = Fixture::new("palette");
    let f = fx.write("p.txt", "text\n");

    let t = Tmux::start("palette", &fx.0, &f);
    t.wait_for("Ln 1, Col 1");
    t.literal("new ");
    t.wait_for("●");
    t.keys(&["C-p"]);
    t.wait_for(" Commands ");
    t.literal("sav");
    t.keys(&["Enter"]);
    t.wait_for("Saved p.txt");
    fx.wait_disk("p.txt", "new text\n");
    assert!(!t.screen().contains(" Commands "), "palette still open:\n{}", t.screen());
}
