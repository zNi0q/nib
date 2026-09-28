use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let d = std::env::temp_dir().join(format!("nib-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("bin")).unwrap();
        Sandbox(d)
    }

    fn tool(&self, name: &str, script: &str) {
        let p = self.0.join("bin").join(name);
        fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn nib(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_nib"))
            .args(args)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.0.join("bin").display()))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        (out.status.code().unwrap_or(-1), text)
    }

    fn plugin(&self, name: &str) -> PathBuf {
        self.0.join("config/nib/plugins").join(format!("{name}.toml"))
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn install_skips_download_when_server_exists_and_adds_plugin() {
    let sb = Sandbox::new("exists");
    sb.tool("gopls", "exit 0");
    let (code, out) = sb.nib(&["plugin", "install", "go"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("already installed"), "{out}");
    assert!(out.contains("ready:"), "{out}");
    assert!(fs::read_to_string(sb.plugin("go")).unwrap().contains("gopls"));
    let (code, out) = sb.nib(&["plugin", "install", "go"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn install_runs_the_install_command_with_the_tool() {
    let sb = Sandbox::new("runs");
    let gobin = sb.0.join("go/bin");
    sb.tool(
        "go",
        &format!("mkdir -p {g} && printf '#!/bin/sh\\n' > {g}/sqls && chmod +x {g}/sqls && echo \"fake go $*\"", g = gobin.display()),
    );
    let (code, out) = sb.nib(&["plugin", "install", "sql"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("$ go install github.com/sqls-server/sqls@latest"), "command shown:\n{out}");
    assert!(out.contains("fake go install"), "{out}");
    assert!(out.contains(&format!("ready: {}", gobin.join("sqls").display())), "{out}");
    assert!(sb.plugin("sql").exists());
}

#[test]
fn install_reports_missing_tool_and_unknown_preset() {
    let sb = Sandbox::new("missing");
    if Path::new("/usr/bin/npm").exists() || Path::new("/bin/npm").exists() {
        sb.tool("npm", "exit 7");
        let (code, out) = sb.nib(&["plugin", "install", "python"]);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("install failed") && out.contains("Not installed: python"), "{out}");
    }
    assert!(!sb.plugin("python").exists(), "no plugin file for a failed install");

    let (code, out) = sb.nib(&["plugin", "install", "cobol"]);
    assert_eq!(code, 1);
    assert!(out.contains("Unknown preset \"cobol\"") && out.contains("typescript"), "{out}");
}

#[test]
fn npm_installs_without_root_go_to_home() {
    let sb = Sandbox::new("npm");
    sb.tool(
        "npm",
        &format!(
            "if [ \"$1\" = prefix ]; then echo /proc/nope; exit 0; fi\n\
             echo \"fake npm $*\"\n\
             mkdir -p {h}/.local/bin && printf '#!/bin/sh\\n' > {h}/.local/bin/pyright-langserver && chmod +x {h}/.local/bin/pyright-langserver",
            h = sb.0.display()
        ),
    );
    let (code, out) = sb.nib(&["plugin", "install", "python"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("npm i -g pyright --prefix {}/.local", sb.0.display())), "{out}");
    assert!(out.contains("ready:"), "{out}");
}

#[test]
fn vue_preset_installs_into_its_own_folder() {
    let sb = Sandbox::new("vue");
    let (code, out) = sb.nib(&["plugin", "add", "vue"]);
    assert_eq!(code, 0, "{out}");
    let body = fs::read_to_string(sb.plugin("vue")).unwrap();
    let data = sb.0.join("data/nib/vue");
    assert!(body.contains(&format!("command = \"{}/bin/vue-language-server\"", data.display())), "{body}");
    assert!(body.contains(&format!("tsdk = \"{}/lib/node_modules/typescript/lib\"", data.display())), "{body}");
    assert!(body.contains("@vue/language-server@2 typescript@5"), "{body}");
}

#[test]
fn rustup_stand_ins_are_skipped() {
    let sb = Sandbox::new("rustup");
    sb.tool("rustup", "[ \"$1\" = which ] && exit 1; exit 0");
    std::os::unix::fs::symlink(sb.0.join("bin/rustup"), sb.0.join("bin/rust-analyzer")).unwrap();
    let (_, out) = sb.nib(&["plugin", "add", "rust"]);
    assert!(out.contains("Added rust"), "{out}");
    let (_, out) = sb.nib(&["plugin", "list"]);
    let line = out.lines().find(|l| l.starts_with("rust")).unwrap();
    assert!(!line.contains(&sb.0.join("bin").display().to_string()), "stand-in picked: {line}");
}

#[test]
fn install_script_checks_arguments() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh");
    let run = |args: &[&str]| {
        let o = Command::new("sh").arg(script).args(args).output().unwrap();
        (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
    };
    let (code, out) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("--lsp typescript,python"), "{out}");
    assert_eq!(run(&["--bogus"]).0, 2);
    let (code, out) = run(&["--lsp"]);
    assert_eq!(code, 2);
    assert!(out.contains("--lsp needs a value"), "{out}");
}

#[test]
#[ignore]
fn install_script_end_to_end() {
    let sb = Sandbox::new("script");
    sb.tool("gopls", "exit 0");
    let real_home = std::env::var("HOME").unwrap();
    let out = Command::new("sh")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh"))
        .args(["--lsp", "go"])
        .env("HOME", &sb.0)
        .env("RUSTUP_HOME", std::env::var("RUSTUP_HOME").unwrap_or(format!("{real_home}/.rustup")))
        .env("CARGO_HOME", std::env::var("CARGO_HOME").unwrap_or(format!("{real_home}/.cargo")))
        .env("CARGO_INSTALL_ROOT", sb.0.join("cargo"))
        .env("XDG_CONFIG_HOME", sb.0.join("config"))
        .env("PATH", format!("{}:/usr/bin:/bin", sb.0.join("bin").display()))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{text}");
    assert!(sb.0.join("cargo/bin/nib").exists(), "{text}");
    assert!(sb.0.join("config/nib/config.nib").exists(), "{text}");
    assert!(sb.plugin("go").exists(), "{text}");
    assert!(text.contains("Done."), "{text}");
}
