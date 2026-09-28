//! LSP plugins: small TOML files that say which language server to run for
//! which files. They are plain data (no scripting runtime), so an installed
//! plugin costs nothing until a matching file is opened.
//!
//! Location: $XDG_CONFIG_HOME/nib/plugins/*.toml (default ~/.config/nib/plugins).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions (without dot) this server handles.
    pub extensions: Vec<String>,
    /// LSP languageId per extension; defaults to the extension itself.
    #[serde(default)]
    pub language_ids: HashMap<String, String>,
    /// Files/folders that mark the project root (searched upwards).
    #[serde(default = "default_roots")]
    pub root_markers: Vec<String>,
    /// Seconds without an open file of this type before the server is stopped.
    #[serde(default = "default_idle")]
    pub idle_timeout: u64,
    /// Extra environment for the server, e.g. NODE_OPTIONS memory caps.
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub init_options: Option<toml::Value>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Install hint shown when the command isn't found.
    #[serde(default)]
    pub install: Option<String>,
}

fn default_roots() -> Vec<String> {
    vec![".git".into()]
}
fn default_idle() -> u64 {
    120
}
fn yes() -> bool {
    true
}

impl Plugin {
    pub fn handles(&self, ext: &str) -> bool {
        self.enabled && self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }

    pub fn language_id(&self, ext: &str) -> String {
        self.language_ids.get(ext).cloned().unwrap_or_else(|| ext.to_string())
    }
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("nib").join("plugins")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// All plugins in the plugin folder, plus a warning per file that failed to parse.
pub fn load() -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    let Ok(rd) = fs::read_dir(dir()) else { return (plugins, warnings) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        match fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|s| toml::from_str::<Plugin>(&s).map_err(|e| e.message().to_string())) {
            Ok(p) => plugins.push(p),
            Err(e) => warnings.push(format!("Plugin {name}: {e}")),
        }
    }
    (plugins, warnings)
}

/// Find an executable: PATH first, then common per-user install folders
/// (Mason, Go, Cargo, Bun, npm-global, ~/.local/bin).
pub fn resolve_command(cmd: &str) -> Option<PathBuf> {
    let p = Path::new(cmd);
    if p.components().count() > 1 {
        return is_exec(p).then(|| p.to_path_buf());
    }
    let h = home();
    let extra = [
        h.join(".local/share/nvim/mason/bin"),
        h.join("go/bin"),
        h.join(".cargo/bin"),
        h.join(".bun/bin"),
        h.join(".npm-global/bin"),
        h.join(".local/bin"),
    ];
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).chain(extra).map(|d| d.join(cmd)).find(|c| is_exec(c))
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// ---------- built-in presets ----------

const NODE_CAP: &str = r#"env = { NODE_OPTIONS = "--max-old-space-size=1024" }"#;

pub fn presets() -> Vec<(&'static str, String)> {
    let p = |name, body: &str| (name, body.trim_start().replace("{NODE_CAP}", NODE_CAP));
    vec![
        p("typescript", r#"
# TypeScript, JavaScript and React (JSX/TSX), using TypeScript 7's native
# language server (Go, ~90 MB; no Node/tsserver). Needs TypeScript >= 7.
name = "typescript"
command = "tsc"
args = ["--lsp", "--stdio"]
extensions = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"]
language_ids = { ts = "typescript", tsx = "typescriptreact", js = "javascript", jsx = "javascriptreact", mjs = "javascript", cjs = "javascript", mts = "typescript", cts = "typescript" }
root_markers = ["tsconfig.json", "jsconfig.json", "package.json", ".git"]
idle_timeout = 120
install = "npm i -g typescript@latest"
"#),
        p("vue", r#"
# Vue single-file components (.vue).
name = "vue"
command = "vue-language-server"
args = ["--stdio"]
extensions = ["vue"]
root_markers = ["package.json", ".git"]
idle_timeout = 120
init_options = { vue = { hybridMode = false } }
{NODE_CAP}
install = "npm i -g @vue/language-server typescript"
"#),
        p("svelte", r#"
name = "svelte"
command = "svelteserver"
args = ["--stdio"]
extensions = ["svelte"]
root_markers = ["svelte.config.js", "package.json", ".git"]
idle_timeout = 120
{NODE_CAP}
install = "npm i -g svelte-language-server"
"#),
        p("html", r#"
name = "html"
command = "vscode-html-language-server"
args = ["--stdio"]
extensions = ["html", "htm"]
root_markers = ["package.json", ".git"]
idle_timeout = 60
init_options = { provideFormatter = true }
{NODE_CAP}
install = "npm i -g vscode-langservers-extracted"
"#),
        p("css", r#"
name = "css"
command = "vscode-css-language-server"
args = ["--stdio"]
extensions = ["css", "scss", "less"]
root_markers = ["package.json", ".git"]
idle_timeout = 60
{NODE_CAP}
install = "npm i -g vscode-langservers-extracted"
"#),
        p("json", r#"
name = "json"
command = "vscode-json-language-server"
args = ["--stdio"]
extensions = ["json", "jsonc"]
root_markers = ["package.json", ".git"]
idle_timeout = 60
{NODE_CAP}
install = "npm i -g vscode-langservers-extracted"
"#),
        p("python", r#"
name = "python"
command = "pyright-langserver"
args = ["--stdio"]
extensions = ["py", "pyi"]
root_markers = ["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt", ".git"]
idle_timeout = 120
{NODE_CAP}
install = "npm i -g pyright"
"#),
        p("sql", r#"
name = "sql"
command = "sqls"
extensions = ["sql"]
root_markers = [".git"]
idle_timeout = 60
install = "go install github.com/sqls-server/sqls@latest"
"#),
        p("rust", r#"
name = "rust"
command = "rust-analyzer"
extensions = ["rs"]
root_markers = ["Cargo.toml", ".git"]
idle_timeout = 180
install = "rustup component add rust-analyzer"
"#),
        p("go", r#"
name = "go"
command = "gopls"
extensions = ["go"]
root_markers = ["go.mod", "go.work", ".git"]
idle_timeout = 120
install = "go install golang.org/x/tools/gopls@latest"
"#),
    ]
}

const TEMPLATE: &str = r#"# nib language-server plugin. Docs: fields below; delete what you don't need.
name = "{name}"
# The server executable and its arguments (most servers need "--stdio").
command = "{name}-language-server"
args = ["--stdio"]
# File extensions this server handles (no dot).
extensions = ["{name}"]
# Optional LSP languageId per extension (defaults to the extension).
# language_ids = { ext = "languageid" }
# Where the project root is: first folder upwards containing one of these.
root_markers = [".git"]
# Stop the server this many seconds after its last file is closed (saves RAM).
idle_timeout = 120
# Extra environment, e.g. cap Node.js servers' memory:
# env = { NODE_OPTIONS = "--max-old-space-size=512" }
# Sent as initializationOptions:
# init_options = { }
# Shown when the command is missing:
# install = "npm i -g ..."
enabled = true
"#;

/// `nib plugin …`. Returns the process exit code.
pub fn cli(args: &[String]) -> i32 {
    let dir = dir();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let names = || presets().iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    match (arg(0), arg(1)) {
        (Some("list") | None, _) => {
            let (plugins, warnings) = load();
            if plugins.is_empty() {
                println!("No plugins installed in {}", dir.display());
            }
            for p in &plugins {
                let found = resolve_command(&p.command).map_or("NOT FOUND".to_string(), |x| x.display().to_string());
                println!("{:<12} {:<4} {} ({found})", p.name, if p.enabled { "on" } else { "off" }, p.command);
                if found == "NOT FOUND" {
                    if let Some(i) = &p.install {
                        println!("             install: {i}");
                    }
                }
            }
            for w in warnings {
                println!("warning: {w}");
            }
            println!("Presets: {}", names());
            0
        }
        (Some("presets"), _) => {
            println!("{}", names());
            0
        }
        (Some("path"), _) => {
            println!("{}", dir.display());
            0
        }
        (Some("add"), Some(_)) => {
            let mut code = 0;
            for name in &args[1..] {
                let Some((_, body)) = presets().into_iter().find(|(n, _)| n == name) else {
                    eprintln!("Unknown preset {name:?}. Presets: {}", names());
                    code = 1;
                    continue;
                };
                code = code.max(write_new(&dir, name, &body, "Added"));
            }
            code
        }
        (Some("new"), Some(name)) => write_new(&dir, name, &TEMPLATE.replace("{name}", name), "Created"),
        (Some("remove"), Some(name)) => {
            let f = dir.join(format!("{name}.toml"));
            match fs::remove_file(&f) {
                Ok(()) => {
                    println!("Removed {}", f.display());
                    0
                }
                Err(e) => {
                    eprintln!("Can't remove {}: {e}", f.display());
                    1
                }
            }
        }
        (Some(cmd @ ("enable" | "disable")), Some(name)) => set_enabled(&dir.join(format!("{name}.toml")), cmd == "enable"),
        _ => {
            eprintln!(
                "usage: nib plugin [list | presets | path | add <preset>... | new <name> | remove <name> | enable <name> | disable <name>]"
            );
            2
        }
    }
}

fn write_new(dir: &Path, name: &str, body: &str, verb: &str) -> i32 {
    let f = dir.join(format!("{name}.toml"));
    if f.exists() {
        eprintln!("{} already exists", f.display());
        return 1;
    }
    if let Err(e) = fs::create_dir_all(dir).and_then(|_| fs::write(&f, body)) {
        eprintln!("Can't write {}: {e}", f.display());
        return 1;
    }
    println!("{verb} {name}: {}", f.display());
    0
}

fn set_enabled(f: &Path, on: bool) -> i32 {
    let Ok(text) = fs::read_to_string(f) else {
        eprintln!("No plugin at {}", f.display());
        return 1;
    };
    let line = format!("enabled = {on}");
    let mut replaced = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("enabled") && l.contains('=') {
                replaced = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !replaced {
        out.push(line);
    }
    match fs::write(f, out.join("\n") + "\n") {
        Ok(()) => {
            println!("{} {}", if on { "Enabled" } else { "Disabled" }, f.display());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_parses() {
        for (name, body) in presets() {
            let p: Plugin = toml::from_str(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(p.name, name);
            assert!(p.install.is_some(), "{name} has an install hint");
            assert!(!p.extensions.is_empty());
        }
        let t: Plugin = toml::from_str(&TEMPLATE.replace("{name}", "x")).unwrap();
        assert_eq!(t.idle_timeout, 120);
    }

    #[test]
    fn defaults_and_language_ids() {
        let p: Plugin = toml::from_str("name='a'\ncommand='a'\nextensions=['ts','x']\nlanguage_ids={ts='typescript'}").unwrap();
        assert_eq!(p.root_markers, [".git"]);
        assert_eq!(p.idle_timeout, 120);
        assert!(p.enabled && p.handles("TS") && !p.handles("rs"));
        assert_eq!(p.language_id("ts"), "typescript");
        assert_eq!(p.language_id("x"), "x");
        assert!(toml::from_str::<Plugin>("name='a'\ncommand='a'\nextensions=[]\ntypo=1").is_err(), "unknown keys are errors");
    }

    #[test]
    fn resolves_commands() {
        assert!(resolve_command("sh").is_some());
        assert!(resolve_command("definitely-not-a-real-lsp").is_none());
        assert_eq!(resolve_command("/bin/sh"), Some(PathBuf::from("/bin/sh")));
    }
}
