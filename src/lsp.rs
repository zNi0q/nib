//! Minimal LSP client, built to cost as little as possible:
//! - a server starts only when a file of its type is opened, and one server is
//!   shared per (plugin, project root);
//! - it is shut down `idle_timeout` seconds after its last file closes;
//! - edits are sent as one debounced full-text sync, not per keystroke;
//! - a reader thread per server blocks on its stdout and wakes the editor
//!   through the same channel as the keyboard, so nothing polls;
//! - stored diagnostics and completion lists are capped; stderr is discarded.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::buffer::Buffer;
use crate::plugin::{resolve_command, Plugin};

/// didChange is sent this long after the last edit.
pub const CHANGE_DELAY: Duration = Duration::from_millis(300);
const MAX_DIAGS: usize = 1000;
const MAX_ITEMS: usize = 200;
const MAX_HOVER: usize = 4000;

/// Everything that can wake the main loop.
pub enum Wake {
    Term(crossterm::event::Event),
    /// A message from server `id`; `None` when the server exited.
    Lsp(usize, Option<Value>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub line: usize,
    /// UTF-16 column, as sent by the server.
    pub col16: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
    pub col16: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub detail: String,
    pub insert: String,
    pub filter: String,
    /// Start (line, UTF-16 col) of the text the server wants replaced.
    pub edit_start: Option<(usize, usize)>,
}

#[derive(Debug, PartialEq)]
pub enum Reply {
    Definition(Vec<Location>),
    Hover(String),
    Completion(Vec<Item>),
    Message(String),
}

enum Req {
    Initialize,
    /// Pull diagnostics (LSP 3.17 `textDocument/diagnostic`) for this file.
    Diagnostic(PathBuf),
    Definition,
    Hover,
    Completion,
    Other,
}

struct Server {
    id: usize,
    plugin: Plugin,
    root: PathBuf,
    child: Child,
    stdin: ChildStdin,
    next_req: i64,
    pending: HashMap<i64, Req>,
    ready: bool,
    /// Messages to send once `initialize` has been answered.
    queue: Vec<Value>,
    open: HashSet<String>,
    idle_since: Option<Instant>,
    triggers: Vec<char>,
    /// Server wants diagnostics requested (pull) rather than pushing them.
    pull_diags: bool,
}

impl Server {
    fn write(&mut self, v: &Value) {
        let body = v.to_string();
        // A dead server shows up as EOF on the reader thread; ignore write errors here.
        let _ = write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).and_then(|_| self.stdin.flush());
    }

    fn send(&mut self, v: Value) {
        if self.ready { self.write(&v) } else { self.queue.push(v) }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn request(&mut self, method: &str, params: Value, req: Req) {
        self.next_req += 1;
        self.pending.insert(self.next_req, req);
        let msg = json!({"jsonrpc": "2.0", "id": self.next_req, "method": method, "params": params});
        if matches!(self.pending[&self.next_req], Req::Initialize) { self.write(&msg) } else { self.send(msg) }
    }

    /// Ask politely, then make sure it's gone without blocking the editor.
    fn stop(mut self) {
        self.ready = true;
        self.request("shutdown", Value::Null, Req::Other);
        self.notify("exit", Value::Null);
        let mut child = self.child;
        std::thread::spawn(move || {
            for _ in 0..10 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
        });
    }
}

struct Doc {
    path: PathBuf,
    uri: String,
    server: usize,
    version: i64,
    change_due: Option<Instant>,
}

pub struct Lsp {
    plugins: Vec<Plugin>,
    tx: Sender<Wake>,
    servers: Vec<Server>,
    next_id: usize,
    doc: Option<Doc>,
    diags: HashMap<PathBuf, Vec<Diag>>,
}

impl Lsp {
    pub fn new(plugins: Vec<Plugin>, tx: Sender<Wake>) -> Lsp {
        Lsp { plugins, tx, servers: Vec::new(), next_id: 1, doc: None, diags: HashMap::new() }
    }

    fn server(&self, id: usize) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    fn server_mut(&mut self, id: usize) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    fn doc_server(&mut self) -> Option<(&mut Server, &mut Doc)> {
        let doc = self.doc.as_mut()?;
        let s = self.servers.iter_mut().find(|s| s.id == doc.server)?;
        Some((s, doc))
    }

    pub fn diags_for(&self, path: &Path) -> &[Diag] {
        self.diags.get(path).map_or(&[], Vec::as_slice)
    }

    /// Name of the current file's server and whether it has finished starting.
    pub fn label(&self) -> Option<(&str, bool)> {
        let s = self.server(self.doc.as_ref()?.server)?;
        Some((&s.plugin.name, s.ready))
    }

    pub fn is_trigger(&self, c: char) -> bool {
        self.doc.as_ref().and_then(|d| self.server(d.server)).is_some_and(|s| s.ready && s.triggers.contains(&c))
    }

    /// A file was opened in the editor. Returns a message for the user, if any.
    pub fn open(&mut self, path: &Path, text: &str) -> Option<String> {
        self.close();
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        let plugin = self.plugins.iter().find(|p| p.handles(&ext))?.clone();
        let root = find_root(path, &plugin.root_markers);
        let existing = self.servers.iter().find(|s| s.plugin.name == plugin.name && s.root == root).map(|s| s.id);
        let id = match existing {
            Some(id) => id,
            None => match self.spawn(&plugin, &root) {
                Ok(id) => id,
                Err(msg) => return Some(msg),
            },
        };
        let uri = path_to_uri(path);
        let s = self.server_mut(id).unwrap();
        s.open.insert(uri.clone());
        s.idle_since = None;
        s.notify("textDocument/didOpen", json!({"textDocument": {
            "uri": uri, "languageId": plugin.language_id(&ext), "version": 1, "text": text,
        }}));
        self.doc = Some(Doc { path: path.to_path_buf(), uri, server: id, version: 1, change_due: None });
        self.pull_diagnostics();
        None
    }

    /// For pull-model servers: ask for the current file's diagnostics.
    fn pull_diagnostics(&mut self) {
        let Some((s, d)) = self.doc_server() else { return };
        if s.ready && s.pull_diags {
            let (uri, path) = (d.uri.clone(), d.path.clone());
            s.request("textDocument/diagnostic", json!({"textDocument": {"uri": uri}}), Req::Diagnostic(path));
        }
    }

    pub fn close(&mut self) {
        let Some(doc) = self.doc.take() else { return };
        if let Some(s) = self.server_mut(doc.server) {
            s.notify("textDocument/didClose", json!({"textDocument": {"uri": doc.uri}}));
            s.open.remove(&doc.uri);
            if s.open.is_empty() {
                s.idle_since = Some(Instant::now());
            }
        }
    }

    pub fn changed(&mut self) {
        if let Some(d) = self.doc.as_mut() {
            d.change_due = Some(Instant::now() + CHANGE_DELAY);
        }
    }

    fn flush(&mut self, buf: &Buffer) {
        let Some((s, d)) = self.doc_server() else { return };
        if d.change_due.take().is_none() {
            return;
        }
        d.version += 1;
        let params = json!({
            "textDocument": {"uri": d.uri, "version": d.version},
            "contentChanges": [{"text": buf.text()}],
        });
        s.notify("textDocument/didChange", params);
        self.pull_diagnostics();
    }

    pub fn saved(&mut self, buf: &Buffer) {
        self.flush(buf);
        if let Some((s, d)) = self.doc_server() {
            let uri = d.uri.clone();
            s.notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}));
        }
    }

    /// Next time `tick` has work to do (debounced sync or idle shutdown).
    pub fn deadline(&self) -> Option<Instant> {
        let idle = self
            .servers
            .iter()
            .filter_map(|s| s.idle_since.map(|t| t + Duration::from_secs(s.plugin.idle_timeout)));
        self.doc.as_ref().and_then(|d| d.change_due).into_iter().chain(idle).min()
    }

    pub fn tick(&mut self, buf: Option<&Buffer>) {
        let now = Instant::now();
        if let (Some(b), Some(due)) = (buf, self.doc.as_ref().and_then(|d| d.change_due)) {
            if due <= now {
                self.flush(b);
            }
        }
        let (idle, keep): (Vec<Server>, Vec<Server>) = std::mem::take(&mut self.servers)
            .into_iter()
            .partition(|s| s.idle_since.is_some_and(|t| now >= t + Duration::from_secs(s.plugin.idle_timeout)));
        self.servers = keep;
        for s in idle {
            s.stop();
        }
    }

    fn position_request(&mut self, buf: &Buffer, method: &str, req: Req) -> Result<(), String> {
        self.flush(buf);
        let Some((s, d)) = self.doc_server() else {
            return Err("No language server for this file (see `nib plugin list`)".into());
        };
        let line = &buf.lines[buf.cur.y];
        let params = json!({
            "textDocument": {"uri": d.uri},
            "position": {"line": buf.cur.y, "character": utf16_col(line, buf.cur.x)},
        });
        s.request(method, params, req);
        Ok(())
    }

    pub fn definition(&mut self, buf: &Buffer) -> Result<(), String> {
        self.position_request(buf, "textDocument/definition", Req::Definition)
    }

    pub fn hover(&mut self, buf: &Buffer) -> Result<(), String> {
        self.position_request(buf, "textDocument/hover", Req::Hover)
    }

    pub fn completion(&mut self, buf: &Buffer) -> Result<(), String> {
        self.position_request(buf, "textDocument/completion", Req::Completion)
    }

    pub fn handle(&mut self, id: usize, msg: Option<Value>) -> Option<Reply> {
        let Some(msg) = msg else {
            // Server exited (crashed, or finished after `stop`).
            let i = self.servers.iter().position(|s| s.id == id)?;
            let s = self.servers.remove(i);
            let mut child = s.child;
            let _ = child.kill();
            let _ = child.wait();
            if self.doc.as_ref().is_some_and(|d| d.server == id) {
                self.doc = None;
            }
            return Some(Reply::Message(if s.ready {
                format!("LSP: {} stopped", s.plugin.name)
            } else {
                format!("LSP: {} failed to start — run `{}` in a terminal to see why", s.plugin.name, s.plugin.command)
            }));
        };
        let s = self.server_mut(id)?;
        if let Some(method) = msg.get("method").and_then(Value::as_str) {
            if let Some(req_id) = msg.get("id") {
                // Server → client request: answer so the server never waits on us.
                let result = match method {
                    "workspace/configuration" => {
                        let n = msg.pointer("/params/items").and_then(Value::as_array).map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; n])
                    }
                    "workspace/workspaceFolders" => json!([{"uri": path_to_uri(&s.root), "name": "root"}]),
                    _ => Value::Null,
                };
                s.write(&json!({"jsonrpc": "2.0", "id": req_id, "result": result}));
                return None;
            }
            return match method {
                "textDocument/publishDiagnostics" => {
                    let params = msg.get("params")?;
                    let path = uri_to_path(params.get("uri")?.as_str()?)?;
                    let diags = parse_diagnostics(params.get("diagnostics")?);
                    if diags.is_empty() { self.diags.remove(&path); } else { self.diags.insert(path, diags); }
                    None
                }
                "window/showMessage" if msg.pointer("/params/type").and_then(Value::as_u64) == Some(1) => {
                    let text = msg.pointer("/params/message")?.as_str()?;
                    Some(Reply::Message(format!("LSP: {}", first_line(text))))
                }
                _ => None,
            };
        }
        let rid = msg.get("id")?.as_i64()?;
        let req = s.pending.remove(&rid)?;
        if let Some(err) = msg.get("error") {
            let text = first_line(err.get("message").and_then(Value::as_str).unwrap_or("request failed")).to_string();
            return match req {
                Req::Other | Req::Diagnostic(_) => None,
                Req::Initialize => {
                    // The server refused to start: stop it instead of waiting forever.
                    let i = self.servers.iter().position(|s| s.id == id)?;
                    let s = self.servers.remove(i);
                    let name = s.plugin.name.clone();
                    s.stop();
                    if self.doc.as_ref().is_some_and(|d| d.server == id) {
                        self.doc = None;
                    }
                    Some(Reply::Message(format!("LSP: {name} failed to start: {text}")))
                }
                _ => Some(Reply::Message(format!("LSP: {text}"))),
            };
        }
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        match req {
            Req::Initialize => {
                s.triggers = result
                    .pointer("/capabilities/completionProvider/triggerCharacters")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str()?.chars().next()).collect())
                    .unwrap_or_default();
                s.pull_diags = result.pointer("/capabilities/diagnosticProvider").is_some_and(|v| !v.is_null() && v != &json!(false));
                s.ready = true;
                s.write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
                for m in std::mem::take(&mut s.queue) {
                    s.write(&m);
                }
                if self.doc.as_ref().is_some_and(|d| d.server == id) {
                    self.pull_diagnostics();
                }
                None
            }
            Req::Diagnostic(path) => {
                // "unchanged" reports keep what we have.
                if result.get("kind").and_then(Value::as_str) == Some("full") {
                    let diags = parse_diagnostics(result.get("items").unwrap_or(&Value::Null));
                    if diags.is_empty() { self.diags.remove(&path); } else { self.diags.insert(path, diags); }
                }
                None
            }
            Req::Definition => Some(Reply::Definition(parse_locations(&result))),
            Req::Hover => Some(Reply::Hover(hover_text(&result))),
            Req::Completion => Some(Reply::Completion(parse_completion(&result))),
            Req::Other => None,
        }
    }

    /// One line per running server with the RAM its whole process tree uses.
    pub fn status(&self) -> String {
        if self.servers.is_empty() {
            return "LSP: no servers running".into();
        }
        let parts: Vec<String> = self
            .servers
            .iter()
            .map(|s| {
                let pid = s.child.id();
                let mb = tree_rss_kb(pid) / 1024;
                let state = if s.ready { "" } else { ", starting" };
                format!("{} (pid {pid}, {mb} MB{state})", s.plugin.name)
            })
            .collect();
        format!("LSP: {}", parts.join(" · "))
    }

    pub fn shutdown_all(&mut self) {
        self.doc = None;
        for s in std::mem::take(&mut self.servers) {
            s.stop();
        }
    }

    /// Stop every server; if a file is open, start its server again.
    pub fn restart(&mut self, buf: Option<&Buffer>) -> Option<String> {
        let path = self.doc.as_ref().map(|d| d.path.clone());
        self.shutdown_all();
        match (path, buf) {
            (Some(p), Some(b)) => self.open(&p, &b.text()).or_else(|| Some("LSP: restarted".into())),
            _ => Some("LSP: stopped all servers".into()),
        }
    }

    fn spawn(&mut self, p: &Plugin, root: &Path) -> Result<usize, String> {
        let Some(exe) = resolve_command(&p.command) else {
            let hint = p.install.as_deref().unwrap_or("see `nib plugin list`");
            return Err(format!("LSP: {} not found — install: {hint}", p.command));
        };
        let mut child = Command::new(exe)
            .args(&p.args)
            .envs(&p.env)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("LSP: can't start {}: {e}", p.command))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let id = self.next_id;
        self.next_id += 1;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                match read_message(&mut r) {
                    Ok(v) => {
                        if tx.send(Wake::Lsp(id, Some(v))).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::InvalidData => continue,
                    Err(_) => {
                        let _ = tx.send(Wake::Lsp(id, None));
                        return;
                    }
                }
            }
        });
        let mut s = Server {
            id,
            plugin: p.clone(),
            root: root.to_path_buf(),
            child,
            stdin,
            next_req: 0,
            pending: HashMap::new(),
            ready: false,
            queue: Vec::new(),
            open: HashSet::new(),
            idle_since: None,
            triggers: Vec::new(),
            pull_diags: false,
        };
        let root_uri = path_to_uri(root);
        let init_options = p.init_options.as_ref().and_then(|v| serde_json::to_value(v).ok());
        s.request("initialize", json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "nib", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "rootPath": root,
            "workspaceFolders": [{"uri": root_uri, "name": root.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()}],
            "initializationOptions": init_options,
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "textDocument": {
                    "synchronization": {"didSave": true},
                    "publishDiagnostics": {},
                    "diagnostic": {"dynamicRegistration": false},
                    "hover": {"contentFormat": ["plaintext", "markdown"]},
                    "definition": {"linkSupport": true},
                    "completion": {"completionItem": {"snippetSupport": false}},
                },
                "workspace": {"configuration": true, "workspaceFolders": true},
            },
        }), Req::Initialize);
        self.servers.push(s);
        Ok(id)
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        for mut s in std::mem::take(&mut self.servers) {
            let _ = s.child.kill();
            let _ = s.child.wait();
        }
    }
}

// ---------- protocol helpers ----------

/// Read one `Content-Length`-framed JSON message.
pub fn read_message(r: &mut impl BufRead) -> io::Result<Value> {
    let mut len = None;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let l = line.trim_end();
        if l.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = l.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len.unwrap()];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|_| io::ErrorKind::InvalidData.into())
}

pub fn utf16_col(line: &str, x: usize) -> usize {
    line.chars().take(x).map(char::len_utf16).sum()
}

pub fn char_from_utf16(line: &str, col16: usize) -> usize {
    let mut u = 0;
    for (i, c) in line.chars().enumerate() {
        if u >= col16 {
            return i;
        }
        u += c.len_utf16();
    }
    line.chars().count()
}

pub fn path_to_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(OsString::from_vec(out)))
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// Nearest folder upwards from `file` that contains one of `markers`.
pub fn find_root(file: &Path, markers: &[String]) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("/"));
    dir.ancestors()
        .find(|d| markers.iter().any(|m| d.join(m).exists()))
        .unwrap_or(dir)
        .to_path_buf()
}

fn pos(v: &Value) -> Option<(usize, usize)> {
    Some((v.get("line")?.as_u64()? as usize, v.get("character")?.as_u64()? as usize))
}

pub fn parse_diagnostics(v: &Value) -> Vec<Diag> {
    let mut out: Vec<Diag> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    let (line, col16) = pos(d.pointer("/range/start")?)?;
                    let severity = match d.get("severity").and_then(Value::as_u64).unwrap_or(1) {
                        1 => Severity::Error,
                        2 => Severity::Warning,
                        3 => Severity::Info,
                        _ => Severity::Hint,
                    };
                    let message = first_line(d.get("message")?.as_str()?).to_string();
                    Some(Diag { line, col16, severity, message })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|d| (d.line, d.col16, d.severity));
    out.truncate(MAX_DIAGS);
    out
}

pub fn parse_locations(v: &Value) -> Vec<Location> {
    let one = |v: &Value| -> Option<Location> {
        let uri = v.get("targetUri").or_else(|| v.get("uri"))?.as_str()?;
        let range = v.get("targetSelectionRange").or_else(|| v.get("range")).or_else(|| v.get("targetRange"))?;
        let (line, col16) = pos(range.get("start")?)?;
        Some(Location { path: uri_to_path(uri)?, line, col16 })
    };
    match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        Value::Null => Vec::new(),
        o => one(o).into_iter().collect(),
    }
}

/// Hover contents as plain text (markdown code fences dropped).
pub fn hover_text(v: &Value) -> String {
    fn part(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => o.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
            Value::Array(a) => a.iter().map(part).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }
    }
    let text = part(v.get("contents").unwrap_or(&Value::Null));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("```")).collect();
    let mut s = lines.join("\n").trim().to_string();
    if s.len() > MAX_HOVER {
        let cut = (0..=MAX_HOVER).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// Turn a snippet like `log(${1:msg})$0` into plain text `log(msg)`.
fn strip_snippet(s: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        match cs[i] {
            '\\' if i + 1 < cs.len() => {
                out.push(cs[i + 1]);
                i += 2;
            }
            '$' if cs.get(i + 1) == Some(&'{') => {
                // ${1:default} or ${1}
                let mut j = i + 2;
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
                if cs.get(j) == Some(&':') {
                    j += 1;
                }
                let mut depth = 1;
                while j < cs.len() {
                    match cs[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        c => out.push(c),
                    }
                    j += 1;
                }
                i = j + 1;
            }
            '$' if cs.get(i + 1).is_some_and(char::is_ascii_digit) => {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

pub fn parse_completion(v: &Value) -> Vec<Item> {
    let items = v.as_array().or_else(|| v.get("items").and_then(Value::as_array));
    let mut out: Vec<(String, Item)> = items
        .map(|a| {
            a.iter()
                .filter_map(|it| {
                    let label = it.get("label")?.as_str()?.to_string();
                    let snippet = it.get("insertTextFormat").and_then(Value::as_u64) == Some(2);
                    let edit = it.get("textEdit");
                    let raw = edit
                        .and_then(|e| e.get("newText"))
                        .or_else(|| it.get("insertText"))
                        .and_then(Value::as_str)
                        .unwrap_or(&label);
                    let insert = if snippet { strip_snippet(raw) } else { raw.to_string() };
                    let edit_start = edit.and_then(|e| e.pointer("/range/start").or_else(|| e.pointer("/insert/start"))).and_then(pos);
                    let str_field = |k: &str| it.get(k).and_then(Value::as_str).map(str::to_string);
                    let sort = str_field("sortText").unwrap_or_else(|| label.clone());
                    let item = Item {
                        detail: str_field("detail").map(|d| first_line(&d).to_string()).unwrap_or_default(),
                        filter: str_field("filterText").unwrap_or_else(|| label.clone()),
                        insert,
                        edit_start,
                        label,
                    };
                    Some((sort, item))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().take(MAX_ITEMS).map(|(_, i)| i).collect()
}

/// Resident memory (kB) of `pid` and all its descendants — language servers
/// often run their real work in child processes (e.g. tsserver under node).
pub fn tree_rss_kb(pid: u32) -> u64 {
    let mut parent: HashMap<u32, u32> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Some(p) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            // /proc/<pid>/stat: "pid (comm) state ppid ..." — comm may contain spaces.
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{p}/stat")) {
                if let Some(ppid) = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(1)?.parse().ok()) {
                    parent.insert(p, ppid);
                }
            }
        }
    }
    let in_tree = |mut p: u32| loop {
        if p == pid {
            return true;
        }
        match parent.get(&p) {
            Some(&pp) if pp != 0 && pp != p => p = pp,
            _ => return false,
        }
    };
    parent
        .keys()
        .filter(|&&p| in_tree(p))
        .filter_map(|p| {
            let status = std::fs::read_to_string(format!("/proc/{p}/status")).ok()?;
            status.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_reads_consecutive_messages() {
        let a = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let b = r#"{"jsonrpc":"2.0","method":"x","params":{"s":"é"}}"#;
        let raw = format!("Content-Length: {}\r\nContent-Type: x\r\n\r\n{a}Content-Length: {}\r\n\r\n{b}", a.len(), b.len());
        let mut r = BufReader::new(raw.as_bytes());
        assert_eq!(read_message(&mut r).unwrap()["id"], 1);
        assert_eq!(read_message(&mut r).unwrap()["params"]["s"], "é");
        assert_eq!(read_message(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn uri_roundtrip() {
        for p in ["/home/me/a b/ñ.ts", "/tmp/x#y%z.rs", "/plain/path.go"] {
            let u = path_to_uri(Path::new(p));
            assert!(u.starts_with("file:///") && !u.contains(' '), "{u}");
            assert_eq!(uri_to_path(&u).unwrap(), PathBuf::from(p));
        }
        assert_eq!(uri_to_path("file:///a%20b").unwrap(), PathBuf::from("/a b"));
        assert!(uri_to_path("https://x").is_none());
    }

    #[test]
    fn utf16_columns() {
        let l = "a😀b日";
        assert_eq!(utf16_col(l, 2), 3);
        assert_eq!(utf16_col(l, 4), 5);
        assert_eq!(char_from_utf16(l, 3), 2);
        assert_eq!(char_from_utf16(l, 99), 4);
    }

    #[test]
    fn locations_in_all_shapes() {
        let loc = json!({"uri": "file:///a.ts", "range": {"start": {"line": 3, "character": 4}, "end": {"line": 3, "character": 5}}});
        let want = Location { path: "/a.ts".into(), line: 3, col16: 4 };
        assert_eq!(parse_locations(&loc), [want.clone()]);
        assert_eq!(parse_locations(&json!([loc])), [want.clone()]);
        let link = json!([{"targetUri": "file:///a.ts", "targetRange": {"start": {"line": 0, "character": 0}},
                           "targetSelectionRange": {"start": {"line": 3, "character": 4}}}]);
        assert_eq!(parse_locations(&link), [want]);
        assert!(parse_locations(&Value::Null).is_empty());
    }

    #[test]
    fn hover_contents_flattened() {
        assert_eq!(hover_text(&json!({"contents": {"kind": "markdown", "value": "```ts\nlet x: number\n```\nDocs"}})), "let x: number\nDocs");
        assert_eq!(hover_text(&json!({"contents": ["a", {"language": "go", "value": "func f()"}]})), "a\nfunc f()");
        assert_eq!(hover_text(&json!({"contents": "plain"})), "plain");
        assert_eq!(hover_text(&Value::Null), "");
    }

    #[test]
    fn completion_items() {
        let v = json!({"isIncomplete": false, "items": [
            {"label": "zeta", "sortText": "1"},
            {"label": "log", "insertTextFormat": 2, "insertText": "log(${1:msg})$0", "detail": "fn\nmore", "sortText": "0"},
            {"label": "map", "textEdit": {"range": {"start": {"line": 2, "character": 6}, "end": {"line": 2, "character": 8}}, "newText": "map"}},
        ]});
        let items = parse_completion(&v);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["log", "zeta", "map"]);
        assert_eq!(items[0].insert, "log(msg)");
        assert_eq!(items[0].detail, "fn");
        assert_eq!(items[2].edit_start, Some((2, 6)));
        assert_eq!(parse_completion(&json!([{"label": "a"}]))[0].insert, "a");
        assert_eq!(strip_snippet(r"a\$b${2}c"), "a$bc");
    }

    #[test]
    fn diagnostics_sorted_and_first_line_only() {
        let d = parse_diagnostics(&json!([
            {"range": {"start": {"line": 5, "character": 1}}, "severity": 2, "message": "w"},
            {"range": {"start": {"line": 1, "character": 0}}, "message": "e\ndetails"},
        ]));
        assert_eq!(d[0], Diag { line: 1, col16: 0, severity: Severity::Error, message: "e".into() });
        assert_eq!(d[1].severity, Severity::Warning);
    }

    #[test]
    fn root_is_nearest_marker() {
        let d = std::env::temp_dir().join(format!("nib-root-{}", std::process::id()));
        std::fs::create_dir_all(d.join("app/src")).unwrap();
        std::fs::write(d.join("app/package.json"), "{}").unwrap();
        let markers = vec!["package.json".to_string()];
        assert_eq!(find_root(&d.join("app/src/x.ts"), &markers), d.join("app"));
        assert_eq!(find_root(&d.join("y.ts"), &markers), d);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tree_rss_counts_self() {
        assert!(tree_rss_kb(std::process::id()) > 0);
    }
}
