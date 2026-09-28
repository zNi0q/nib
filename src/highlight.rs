//! Syntax highlighter: hand-written scanners, no grammars or regexes, so it
//! costs almost nothing in RAM or CPU. It works line by line; the only thing
//! carried from one line to the next is a tiny `State` (an open comment or
//! string, the current Vue/Svelte section, an open HTML tag, CSS brace depth).

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Text,
    Keyword,
    Str,
    Comment,
    Number,
    Func,
    Type,
    /// HTML/JSX tag names and brackets, CSS element selectors.
    Tag,
    /// HTML attributes, CSS properties, JSON/YAML/TOML keys.
    Attr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Open {
    #[default]
    None,
    Comment,
    /// Multi-line string closed by this delimiter.
    Str(&'static str),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Section {
    #[default]
    Markup,
    Script,
    Style,
}

/// Highlighter state at the start of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    open: Open,
    section: Section,
    in_tag: bool,
    /// The tag being read is <script>/<style>: switch section when it closes.
    tag_opens: Option<Section>,
    depth: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Code,
    Css,
    Markup,
    Markdown,
    Config,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Html,
    Vue,
    Svelte,
}

pub struct Lang {
    pub name: &'static str,
    kind: Kind,
    flavor: Flavor,
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    /// Delimiters of strings that may span lines (`"""`, backticks).
    multi: &'static [&'static str],
    /// `'` only starts 'x' / '\n' char literals (Rust lifetimes stay plain).
    char_lit: bool,
    kw: &'static [&'static str],
    case_insensitive: bool,
    jsx: bool,
    decorators: bool,
    /// Shell-style `$VAR` / `${VAR}`.
    vars: bool,
    /// C-style `#include` at line start.
    preproc: bool,
    /// A string followed by `:` is a key (JSON).
    json_keys: bool,
}

const BASE: Lang = Lang {
    name: "",
    kind: Kind::Code,
    flavor: Flavor::Html,
    line: &[],
    block: None,
    quotes: &['"'],
    multi: &[],
    char_lit: false,
    kw: &[],
    case_insensitive: false,
    jsx: false,
    decorators: false,
    vars: false,
    preproc: false,
    json_keys: false,
};

const JS_KW: &[&str] = &[
    "abstract", "any", "as", "async", "await", "boolean", "break", "case", "catch", "class", "const", "continue",
    "debugger", "declare", "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally",
    "for", "from", "function", "get", "if", "implements", "import", "in", "infer", "instanceof", "interface",
    "is", "keyof", "let", "namespace", "never", "new", "null", "number", "object", "of", "private", "protected",
    "public", "readonly", "return", "satisfies", "set", "static", "string", "super", "switch", "symbol", "this",
    "throw", "true", "try", "type", "typeof", "undefined", "unknown", "var", "void", "while", "with", "yield",
];
const C_KW: &[&str] = &[
    "auto", "bool", "break", "case", "char", "class", "const", "continue", "default", "delete", "do", "double",
    "else", "enum", "extern", "false", "float", "for", "goto", "if", "inline", "int", "long", "namespace", "new",
    "nullptr", "private", "protected", "public", "return", "short", "signed", "sizeof", "static", "struct",
    "switch", "template", "this", "true", "typedef", "typename", "union", "unsigned", "using", "virtual", "void",
    "volatile", "while", "NULL",
];

static RUST: Lang = Lang {
    name: "Rust", line: &["//"], block: Some(("/*", "*/")), char_lit: true,
    kw: &["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false", "fn",
          "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
          "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "Some",
          "None", "Ok", "Err"],
    ..BASE
};
static GO: Lang = Lang {
    name: "Go", line: &["//"], block: Some(("/*", "*/")), multi: &["`"], char_lit: true,
    kw: &["break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "false", "for",
          "func", "go", "goto", "if", "import", "interface", "iota", "map", "nil", "package", "range", "return",
          "select", "struct", "switch", "true", "type", "var"],
    ..BASE
};
static C: Lang = Lang { name: "C/C++", line: &["//"], block: Some(("/*", "*/")), char_lit: true, preproc: true, kw: C_KW, ..BASE };
static JS: Lang = Lang {
    name: "JavaScript", line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\''], multi: &["`"],
    kw: JS_KW, jsx: true, decorators: true, ..BASE
};
static TS: Lang = Lang { name: "TypeScript", jsx: false, ..JS };
static TSX: Lang = Lang { name: "TSX", ..JS };
static JAVA: Lang = Lang {
    name: "Java-like", line: &["//"], block: Some(("/*", "*/")), multi: &["\"\"\""], char_lit: true, decorators: true,
    kw: &["abstract", "boolean", "break", "case", "catch", "class", "const", "continue", "default", "do", "double",
          "else", "enum", "extends", "false", "final", "finally", "float", "for", "fun", "func", "if", "implements",
          "import", "int", "interface", "let", "long", "namespace", "new", "null", "object", "override", "package",
          "private", "protected", "public", "return", "static", "string", "super", "switch", "this", "throw",
          "throws", "true", "try", "using", "val", "var", "void", "when", "while"],
    ..BASE
};
static PYTHON: Lang = Lang {
    name: "Python", line: &["#"], quotes: &['"', '\''], multi: &["\"\"\"", "'''"], decorators: true,
    kw: &["and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del", "elif",
          "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
          "match", "None", "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try", "while",
          "with", "yield"],
    ..BASE
};
static ZIG: Lang = Lang {
    name: "Zig", line: &["//"], char_lit: true,
    kw: &["and", "break", "catch", "comptime", "const", "continue", "defer", "else", "enum", "errdefer", "error",
          "export", "extern", "false", "fn", "for", "if", "inline", "null", "or", "orelse", "pub", "return",
          "struct", "switch", "test", "true", "try", "undefined", "union", "unreachable", "var", "while"],
    ..BASE
};
static SHELL: Lang = Lang {
    name: "Shell", line: &["#"], quotes: &['"', '\''], vars: true,
    kw: &["case", "do", "done", "echo", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
          "local", "return", "then", "while"],
    ..BASE
};
static LUA: Lang = Lang {
    name: "Lua", line: &["--"], block: Some(("--[[", "]]")), quotes: &['"', '\''],
    kw: &["and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil",
          "not", "or", "repeat", "return", "then", "true", "until", "while"],
    ..BASE
};
static SQL: Lang = Lang {
    name: "SQL", line: &["--"], block: Some(("/*", "*/")), quotes: &['\'', '"'], case_insensitive: true,
    kw: &["add", "all", "alter", "and", "as", "asc", "begin", "between", "bigint", "boolean", "by", "cascade",
          "case", "check", "column", "commit", "constraint", "create", "cross", "date", "decimal", "default",
          "delete", "desc", "distinct", "drop", "else", "end", "exists", "false", "float", "foreign", "from",
          "full", "group", "having", "if", "in", "index", "inner", "insert", "int", "integer", "into", "is",
          "join", "json", "jsonb", "key", "left", "like", "limit", "not", "null", "numeric", "offset", "on", "or",
          "order", "outer", "primary", "real", "references", "returning", "right", "rollback", "select",
          "serial", "set", "table", "text", "then", "timestamp", "timestamptz", "transaction", "true", "union",
          "unique", "update", "uuid", "values", "varchar", "view", "when", "where", "with"],
    ..BASE
};
static CSS: Lang = Lang { name: "CSS", kind: Kind::Css, quotes: &['"', '\''], ..BASE };
static SCSS: Lang = Lang { name: "SCSS", line: &["//"], ..CSS };
static HTML: Lang = Lang { name: "HTML", kind: Kind::Markup, quotes: &['"', '\''], ..BASE };
static VUE: Lang = Lang { name: "Vue", flavor: Flavor::Vue, ..HTML };
static SVELTE: Lang = Lang { name: "Svelte", flavor: Flavor::Svelte, ..HTML };
static JSON: Lang = Lang { name: "JSON", line: &["//"], json_keys: true, kw: &["true", "false", "null"], ..BASE };
static CONFIG: Lang = Lang {
    name: "Config", kind: Kind::Config, line: &["#"], quotes: &['"', '\''], multi: &["\"\"\"", "'''"], vars: true,
    kw: &["true", "false", "null", "yes", "no", "on", "off"],
    ..BASE
};
static MARKDOWN: Lang = Lang { name: "Markdown", kind: Kind::Markdown, ..BASE };

static LANGS: &[(&[&str], &Lang)] = &[
    (&["rs"], &RUST),
    (&["go"], &GO),
    (&["c", "h", "cpp", "cc", "cxx", "hpp", "hh"], &C),
    (&["js", "mjs", "cjs", "jsx"], &JS),
    (&["ts", "mts", "cts"], &TS),
    (&["tsx"], &TSX),
    (&["java", "kt", "kts", "cs", "dart", "swift", "scala"], &JAVA),
    (&["py", "pyi"], &PYTHON),
    (&["zig", "zon"], &ZIG),
    (&["sh", "bash", "zsh", "fish"], &SHELL),
    (&["lua"], &LUA),
    (&["sql"], &SQL),
    (&["css"], &CSS),
    (&["scss", "sass", "less"], &SCSS),
    (&["html", "htm", "xml", "svg", "xhtml"], &HTML),
    (&["vue"], &VUE),
    (&["svelte"], &SVELTE),
    (&["json", "jsonc", "json5"], &JSON),
    (&["toml", "yaml", "yml", "ini", "conf", "cfg", "env", "mk", "dockerfile", "properties", "nib"], &CONFIG),
    (&["md", "markdown"], &MARKDOWN),
];

pub fn lang_for(path: &Path) -> Option<&'static Lang> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let ext = match name.as_str() {
        "makefile" | "gnumakefile" => "mk",
        n if n == "dockerfile" || n.starts_with("dockerfile.") || n.ends_with(".dockerfile") => "dockerfile",
        n if n.starts_with(".env") => "env",
        n if n.starts_with(".bashrc") || n.starts_with(".zshrc") || n == ".profile" => "sh",
        _ => name.rsplit_once('.')?.1,
    };
    LANGS.iter().find(|(exts, _)| exts.contains(&ext)).map(|(_, l)| *l)
}

/// One token kind per char of `line`. `st` is the state at the start of the
/// line and is updated to the state at the start of the next line.
pub fn highlight_line(line: &str, lang: &Lang, st: &mut State) -> Vec<Tok> {
    let cs: Vec<char> = line.chars().collect();
    let mut out = vec![Tok::Text; cs.len()];
    let n = cs.len();
    let mut h = Hl { cs: &cs, out: &mut out, st };
    match lang.kind {
        Kind::Code => h.code(0, n, lang),
        Kind::Css => h.css(0, n, lang),
        Kind::Markup => h.sfc(lang),
        Kind::Markdown => h.markdown(),
        Kind::Config => h.config(lang),
    }
    out
}

/// State at the start of each line.
pub fn line_states(lines: &[String], lang: &Lang) -> Vec<State> {
    let mut st = State::default();
    lines
        .iter()
        .map(|l| {
            let at_start = st;
            highlight_line(l, lang, &mut st);
            at_start
        })
        .collect()
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn css_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

struct Hl<'a> {
    cs: &'a [char],
    out: &'a mut [Tok],
    st: &'a mut State,
}

impl Hl<'_> {
    fn at(&self, i: usize, pat: &str) -> bool {
        let mut j = i;
        pat.chars().all(|p| {
            let ok = self.cs.get(j) == Some(&p);
            j += 1;
            ok
        })
    }

    fn at_ci(&self, i: usize, pat: &str) -> bool {
        let mut j = i;
        pat.chars().all(|p| {
            let ok = self.cs.get(j).is_some_and(|c| c.eq_ignore_ascii_case(&p));
            j += 1;
            ok
        })
    }

    fn find(&self, from: usize, end: usize, pat: &str) -> Option<usize> {
        (from..end).find(|&j| self.at(j, pat))
    }

    /// Like `find`, skipping backslash-escaped characters.
    fn find_unescaped(&self, from: usize, end: usize, pat: &str) -> Option<usize> {
        let mut j = from;
        while j < end {
            if self.cs[j] == '\\' {
                j += 2;
                continue;
            }
            if self.at(j, pat) {
                return Some(j);
            }
            j += 1;
        }
        None
    }

    fn fill(&mut self, a: usize, b: usize, t: Tok) {
        let b = b.min(self.out.len());
        if a < b {
            self.out[a..b].fill(t);
        }
    }

    fn next_non_space(&self, j: usize, end: usize) -> Option<char> {
        self.cs[j.min(end)..end].iter().copied().find(|c| !c.is_whitespace())
    }

    fn word_end(&self, i: usize, end: usize, f: fn(char) -> bool) -> usize {
        let mut j = i;
        while j < end && f(self.cs[j]) {
            j += 1;
        }
        j
    }

    /// End (exclusive) of a quoted string starting at `i`, within the line.
    fn string_end(&self, i: usize, end: usize) -> usize {
        let q = self.cs[i];
        let mut j = i + 1;
        while j < end && self.cs[j] != q {
            j += if self.cs[j] == '\\' { 2 } else { 1 };
        }
        (j + 1).min(end)
    }

    /// Index of the `}` matching the `{` at `i` (or `end`).
    fn match_brace(&self, i: usize, end: usize) -> usize {
        let mut depth = 0;
        for j in i..end {
            match self.cs[j] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return j;
                    }
                }
                _ => {}
            }
        }
        end
    }

    /// Finish an open comment/string: color up to and including `close`.
    fn close(&mut self, i: usize, end: usize, close: &str, t: Tok) -> usize {
        match self.find_unescaped(i, end, close) {
            Some(p) => {
                let e = p + close.chars().count();
                self.fill(i, e, t);
                self.st.open = Open::None;
                e
            }
            None => {
                self.fill(i, end, t);
                end
            }
        }
    }

    /// Highlight `{ expr }` embedded in markup with a scratch state.
    fn embedded(&mut self, a: usize, b: usize, lang: &Lang) {
        let mut scratch = State::default();
        let mut h = Hl { cs: self.cs, out: &mut *self.out, st: &mut scratch };
        h.code(a, b, lang);
    }

    // ---------- programming languages ----------

    fn code(&mut self, mut i: usize, end: usize, l: &Lang) {
        if l.jsx && self.st.in_tag {
            i = self.tag_rest(i, end, true, l);
        }
        while i < end {
            match self.st.open {
                Open::Comment => {
                    i = self.close(i, end, l.block.map_or("*/", |b| b.1), Tok::Comment);
                    continue;
                }
                Open::Str(d) => {
                    i = self.close(i, end, d, Tok::Str);
                    continue;
                }
                Open::None => {}
            }
            if let Some((s, _)) = l.block {
                if self.at(i, s) {
                    let n = s.chars().count();
                    self.fill(i, i + n, Tok::Comment);
                    self.st.open = Open::Comment;
                    i += n;
                    continue;
                }
            }
            if l.line.iter().any(|p| self.at(i, p)) {
                self.fill(i, end, Tok::Comment);
                return;
            }
            if let Some(d) = l.multi.iter().find(|d| self.at(i, d)) {
                let n = d.chars().count();
                self.fill(i, i + n, Tok::Str);
                self.st.open = Open::Str(d);
                i += n;
                continue;
            }
            let c = self.cs[i];
            let quoted = l.quotes.contains(&c)
                || (c == '\'' && l.char_lit && (self.cs.get(i + 1) == Some(&'\\') || self.cs.get(i + 2) == Some(&'\'')));
            if quoted {
                let e = self.string_end(i, end);
                let t = if l.json_keys && self.next_non_space(e, end) == Some(':') { Tok::Attr } else { Tok::Str };
                self.fill(i, e, t);
                i = e;
                continue;
            }
            if c == '<' && l.jsx && self.jsx_tag_start(i) {
                i = self.tag_open(i, end, true, l);
                continue;
            }
            let next_alpha = self.cs.get(i + 1).is_some_and(|c| c.is_alphabetic() || *c == '_');
            if c == '@' && l.decorators && next_alpha {
                let e = self.word_end(i + 1, end, ident);
                self.fill(i, e, Tok::Func);
                i = e;
                continue;
            }
            if c == '$' && l.vars && (next_alpha || self.cs.get(i + 1) == Some(&'{')) {
                let e = if self.cs[i + 1] == '{' {
                    self.find(i, end, "}").map_or(end, |p| p + 1)
                } else {
                    self.word_end(i + 1, end, ident)
                };
                self.fill(i, e, Tok::Attr);
                i = e;
                continue;
            }
            if c == '#' && l.preproc && self.cs[..i].iter().all(|c| c.is_whitespace()) {
                let e = self.word_end(i + 1, end, ident);
                self.fill(i, e, Tok::Keyword);
                i = e;
                continue;
            }
            if c.is_ascii_digit() && (i == 0 || !ident(self.cs[i - 1])) {
                let e = self.word_end(i, end, |c| ident(c) || c == '.');
                self.fill(i, e, Tok::Number);
                i = e;
                continue;
            }
            if ident(c) {
                let e = self.word_end(i, end, ident);
                let word: String = self.cs[i..e].iter().collect();
                // Python string prefixes: f"..", r'..', b"..".
                if l.name == "Python" && e - i <= 2 && word.chars().all(|c| "fFrRbBuU".contains(c))
                    && matches!(self.cs.get(e), Some('"') | Some('\''))
                {
                    self.fill(i, e, Tok::Str);
                    i = e;
                    continue;
                }
                let is_kw = if l.case_insensitive {
                    l.kw.iter().any(|k| k.eq_ignore_ascii_case(&word))
                } else {
                    l.kw.contains(&word.as_str())
                };
                let t = if is_kw {
                    Tok::Keyword
                } else if matches!(self.next_non_space(e, end), Some('(') | Some('!')) {
                    Tok::Func
                } else if c.is_uppercase() {
                    Tok::Type
                } else {
                    Tok::Text
                };
                self.fill(i, e, t);
                i = e;
                continue;
            }
            i += 1;
        }
    }

    /// Is the `<` at `i` the start of a JSX tag (not a less-than)?
    fn jsx_tag_start(&self, i: usize) -> bool {
        let next = self.cs.get(i + 1).copied();
        if !next.is_some_and(|c| c.is_alphabetic() || c == '/' || c == '>') {
            return false;
        }
        let Some(p) = self.cs[..i].iter().rposition(|c| !c.is_whitespace()) else { return true };
        let pc = self.cs[p];
        if ident(pc) {
            let start = self.cs[..=p].iter().rposition(|c| !ident(*c)).map_or(0, |s| s + 1);
            let word: String = self.cs[start..=p].iter().collect();
            return matches!(word.as_str(), "return" | "yield" | "default" | "await");
        }
        "(=,:?&|{[;>!".contains(pc)
    }

    // ---------- markup (HTML, Vue, Svelte, JSX tags) ----------

    /// `<name` … : color the tag name, then read attributes.
    fn tag_open(&mut self, i: usize, end: usize, braces: bool, inner: &Lang) -> usize {
        let mut j = i + 1;
        let closing = self.cs.get(j) == Some(&'/');
        if closing {
            j += 1;
        }
        if self.cs.get(j) == Some(&'!') {
            let e = self.find(j, end, ">").map_or(end, |p| p + 1);
            self.fill(i, e, Tok::Keyword);
            return e;
        }
        let ns = j;
        let ne = self.word_end(ns, end, |c| css_ident(c) || c == ':' || c == '.');
        self.fill(i, ns, Tok::Tag);
        let t = if self.cs.get(ns).is_some_and(|c| c.is_uppercase()) { Tok::Type } else { Tok::Tag };
        self.fill(ns, ne, t);
        let name: String = self.cs[ns..ne].iter().collect::<String>().to_lowercase();
        self.st.tag_opens = match name.as_str() {
            _ if closing => None,
            "script" => Some(Section::Script),
            "style" => Some(Section::Style),
            _ => None,
        };
        self.st.in_tag = true;
        self.tag_rest(ne, end, braces, inner)
    }

    /// Attributes up to the closing `>` (which may be on a later line).
    fn tag_rest(&mut self, mut i: usize, end: usize, braces: bool, inner: &Lang) -> usize {
        while i < end {
            let c = self.cs[i];
            if c.is_whitespace() || c == '=' {
                i += 1;
            } else if self.at(i, "/>") {
                self.fill(i, i + 2, Tok::Tag);
                self.st.in_tag = false;
                self.st.tag_opens = None;
                return i + 2;
            } else if c == '>' {
                self.fill(i, i + 1, Tok::Tag);
                self.st.in_tag = false;
                return i + 1;
            } else if c == '"' || c == '\'' {
                let e = self.string_end(i, end);
                self.fill(i, e, Tok::Str);
                i = e;
            } else if c == '{' && braces {
                let e = self.match_brace(i, end);
                self.fill(i, i + 1, Tok::Keyword);
                self.embedded(i + 1, e, inner);
                self.fill(e, e + 1, Tok::Keyword);
                i = e + 1;
            } else {
                let e = self.word_end(i, end, |c| !c.is_whitespace() && !"=>/\"'{".contains(c)).max(i + 1);
                let name: String = self.cs[i..e].iter().collect();
                let directive = ["v-", ":", "@", "#", "on:", "bind:", "use:", "class:"].iter().any(|p| name.starts_with(p));
                self.fill(i, e, if directive { Tok::Keyword } else { Tok::Attr });
                i = e;
            }
        }
        end
    }

    /// Markup text and tags. Returns early (at the new position) when a
    /// <script>/<style> tag closes, so `sfc` can switch language.
    fn markup(&mut self, mut i: usize, end: usize, l: &Lang) -> usize {
        let braces = l.flavor == Flavor::Svelte;
        while i < end {
            if self.st.open == Open::Comment {
                i = self.close(i, end, "-->", Tok::Comment);
                continue;
            }
            if self.st.in_tag {
                i = self.tag_rest(i, end, braces, &TS);
                if let Some(s) = self.section_switch() {
                    self.st.section = s;
                    return i;
                }
                continue;
            }
            if self.at(i, "<!--") {
                self.fill(i, i + 4, Tok::Comment);
                self.st.open = Open::Comment;
                i += 4;
                continue;
            }
            let c = self.cs[i];
            if c == '<' && self.cs.get(i + 1).is_some_and(|c| c.is_alphabetic() || *c == '/' || *c == '!') {
                i = self.tag_open(i, end, braces, &TS);
                if let Some(s) = self.section_switch() {
                    self.st.section = s;
                    return i;
                }
                continue;
            }
            if c == '&' {
                let e = self.word_end(i + 1, end, |c| c.is_alphanumeric() || c == '#');
                if self.cs.get(e) == Some(&';') && e > i + 1 {
                    self.fill(i, e + 1, Tok::Number);
                    i = e + 1;
                    continue;
                }
            }
            if l.flavor == Flavor::Vue && self.at(i, "{{") {
                let e = self.find(i + 2, end, "}}").unwrap_or(end);
                self.fill(i, i + 2, Tok::Keyword);
                self.embedded(i + 2, e, &TS);
                self.fill(e, e + 2, Tok::Keyword);
                i = e + 2;
                continue;
            }
            if l.flavor == Flavor::Svelte && c == '{' {
                let e = self.match_brace(i, end);
                self.fill(i, i + 1, Tok::Keyword);
                let mut j = i + 1;
                if self.cs.get(j).is_some_and(|c| "#/:@".contains(*c)) {
                    j = self.word_end(j + 1, e, ident);
                    self.fill(i + 1, j, Tok::Keyword);
                }
                self.embedded(j, e, &TS);
                self.fill(e, e + 1, Tok::Keyword);
                i = e + 1;
                continue;
            }
            i += 1;
        }
        end
    }

    /// After a tag closes: did it open a <script> or <style> section?
    fn section_switch(&mut self) -> Option<Section> {
        if self.st.in_tag { None } else { self.st.tag_opens.take() }
    }

    /// HTML / Vue / Svelte: markup with embedded script and style sections.
    fn sfc(&mut self, l: &Lang) {
        let n = self.cs.len();
        let mut i = 0;
        while i < n {
            match self.st.section {
                Section::Markup => i = self.markup(i, n, l),
                Section::Script | Section::Style => {
                    let script = self.st.section == Section::Script;
                    let close = if script { "</script" } else { "</style" };
                    let p = (i..n).find(|&j| self.at_ci(j, close)).unwrap_or(n);
                    if script { self.code(i, p, &TS) } else { self.css(i, p, &SCSS) }
                    if p < n {
                        self.st.section = Section::Markup;
                        self.st.open = Open::None;
                        self.st.depth = 0;
                    }
                    i = p;
                }
            }
        }
    }

    // ---------- CSS / SCSS ----------

    fn css(&mut self, mut i: usize, end: usize, l: &Lang) {
        let last_brace = self.cs[..end].iter().rposition(|c| *c == '{');
        while i < end {
            if self.st.open == Open::Comment {
                i = self.close(i, end, "*/", Tok::Comment);
                continue;
            }
            if self.at(i, "/*") {
                self.fill(i, i + 2, Tok::Comment);
                self.st.open = Open::Comment;
                i += 2;
                continue;
            }
            if l.line.iter().any(|p| self.at(i, p)) && (i == 0 || self.cs[i - 1] != ':') {
                self.fill(i, end, Tok::Comment);
                return;
            }
            let c = self.cs[i];
            match c {
                '"' | '\'' => {
                    let e = self.string_end(i, end);
                    self.fill(i, e, Tok::Str);
                    i = e;
                    continue;
                }
                '{' => { self.st.depth = self.st.depth.saturating_add(1); i += 1; continue; }
                '}' => { self.st.depth = self.st.depth.saturating_sub(1); i += 1; continue; }
                '@' => {
                    let e = self.word_end(i + 1, end, css_ident);
                    self.fill(i, e, Tok::Keyword);
                    i = e;
                    continue;
                }
                _ => {}
            }
            let selector = self.st.depth == 0 || last_brace.is_some_and(|p| i < p);
            let next_ident = self.cs.get(i + 1).is_some_and(|c| css_ident(*c));
            if selector {
                if (c == '.' || c == '#') && next_ident {
                    let e = self.word_end(i + 1, end, css_ident);
                    self.fill(i, e, Tok::Type);
                    i = e;
                } else if c == ':' {
                    let s = self.word_end(i, end, |c| c == ':');
                    let e = self.word_end(s, end, css_ident);
                    self.fill(i, e, Tok::Func);
                    i = e;
                } else if css_ident(c) && !c.is_ascii_digit() {
                    let e = self.word_end(i, end, css_ident);
                    self.fill(i, e, Tok::Tag);
                    i = e;
                } else {
                    if c == '&' || c == '*' || c == '>' || c == '+' || c == '~' {
                        self.fill(i, i + 1, Tok::Keyword);
                    }
                    i += 1;
                }
                continue;
            }
            let digit_next = self.cs.get(i + 1).is_some_and(|c| c.is_ascii_digit());
            if c == '#' && next_ident {
                let e = self.word_end(i + 1, end, |c| c.is_ascii_hexdigit());
                self.fill(i, e, Tok::Number);
                i = e;
            } else if c.is_ascii_digit() || ((c == '.' || c == '-') && digit_next && (i == 0 || !css_ident(self.cs[i - 1]))) {
                let e = self.word_end(i + 1, end, |c| c.is_alphanumeric() || c == '.' || c == '%');
                self.fill(i, e, Tok::Number);
                i = e;
            } else if c == '!' && next_ident {
                let e = self.word_end(i + 1, end, css_ident);
                self.fill(i, e, Tok::Keyword);
                i = e;
            } else if css_ident(c) {
                let e = self.word_end(i, end, css_ident);
                let t = match self.next_non_space(e, end) {
                    Some(':') => Tok::Attr,
                    Some('(') => Tok::Func,
                    _ => Tok::Text,
                };
                self.fill(i, e, t);
                i = e;
            } else if c == '$' && next_ident {
                let e = self.word_end(i + 1, end, css_ident);
                self.fill(i, e, Tok::Attr);
                i = e;
            } else {
                i += 1;
            }
        }
    }

    // ---------- Markdown ----------

    fn markdown(&mut self) {
        let n = self.cs.len();
        let start = self.word_end(0, n, char::is_whitespace);
        let fence = self.at(start, "```") || self.at(start, "~~~");
        if self.st.open != Open::None {
            if fence {
                self.fill(0, n, Tok::Comment);
                self.st.open = Open::None;
            } else {
                self.fill(0, n, Tok::Str);
            }
            return;
        }
        if fence {
            self.fill(0, n, Tok::Comment);
            self.st.open = Open::Str("```");
            return;
        }
        match self.cs.get(start) {
            Some('#') => return self.fill(0, n, Tok::Keyword),
            Some('>') => return self.fill(0, n, Tok::Comment),
            _ => {}
        }
        let mut i = start;
        if self.cs.get(i).is_some_and(|c| "-*+".contains(*c)) && self.cs.get(i + 1) == Some(&' ') {
            self.fill(i, i + 1, Tok::Keyword);
            i += 2;
        } else {
            let d = self.word_end(i, n, |c| c.is_ascii_digit());
            if d > i && self.cs.get(d) == Some(&'.') {
                self.fill(i, d + 1, Tok::Keyword);
                i = d + 1;
            }
        }
        while i < n {
            let c = self.cs[i];
            if c == '`' {
                let e = self.find(i + 1, n, "`").map_or(n, |p| p + 1);
                self.fill(i, e, Tok::Str);
                i = e;
            } else if self.at(i, "**") || self.at(i, "__") {
                let pat: String = self.cs[i..i + 2].iter().collect();
                let e = self.find(i + 2, n, &pat).map_or(n, |p| p + 2);
                self.fill(i, e, Tok::Type);
                i = e;
            } else if c == '[' {
                match self.find(i, n, "](").and_then(|m| self.find(m, n, ")").map(|e| (m, e))) {
                    Some((m, e)) => {
                        self.fill(i, m + 1, Tok::Func);
                        self.fill(m + 1, e + 1, Tok::Attr);
                        i = e + 1;
                    }
                    None => i += 1,
                }
            } else {
                i += 1;
            }
        }
    }

    // ---------- TOML / YAML / INI / .env / Dockerfile / Makefile ----------

    fn config(&mut self, l: &Lang) {
        let n = self.cs.len();
        if self.st.open != Open::None {
            return self.code(0, n, l);
        }
        let mut i = self.word_end(0, n, char::is_whitespace);
        match self.cs.get(i) {
            None => return,
            Some('#') | Some(';') => return self.fill(i, n, Tok::Comment),
            Some('[') => {
                let e = self.find(i, n, "]").map_or(n, |p| p + 1);
                self.fill(i, e, Tok::Tag);
                return self.code(e, n, l);
            }
            _ => {}
        }
        if self.at(i, "- ") {
            self.fill(i, i + 1, Tok::Keyword);
            i += 2;
        }
        if self.at(i, "export ") {
            self.fill(i, i + 6, Tok::Keyword);
            i += 7;
        }
        let key_end = if matches!(self.cs.get(i), Some('"') | Some('\'')) {
            self.string_end(i, n)
        } else {
            self.word_end(i, n, |c| ident(c) || c == '-' || c == '.')
        };
        if key_end > i && matches!(self.next_non_space(key_end, n), Some(':') | Some('=')) {
            self.fill(i, key_end, Tok::Attr);
            i = key_end;
        } else if key_end >= i + 2
            && self.cs[i..key_end].iter().all(|c| c.is_ascii_uppercase())
            && self.cs.get(key_end) == Some(&' ')
        {
            // Dockerfile instruction (FROM, RUN, COPY …).
            self.fill(i, key_end, Tok::Keyword);
            i = key_end;
        }
        self.code(i, n, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Tok::*;

    /// Token of `needle` (first occurrence) in multi-line `text`, asserting
    /// every char of it got the same token.
    fn tok(path: &str, text: &str, needle: &str) -> Tok {
        let lang = lang_for(Path::new(path)).unwrap_or_else(|| panic!("no lang for {path}"));
        let mut st = State::default();
        let toks: Vec<Vec<Tok>> = text.lines().map(|l| highlight_line(l, lang, &mut st)).collect();
        for (y, line) in text.lines().enumerate() {
            if let Some(b) = line.find(needle) {
                let x = line[..b].chars().count();
                let n = needle.chars().count();
                let ts = &toks[y][x..x + n];
                assert!(ts.iter().all(|t| *t == ts[0]), "{path}: {needle:?} mixed tokens {ts:?} in {line:?}");
                return ts[0];
            }
        }
        panic!("{needle:?} not in text");
    }

    #[test]
    fn rust() {
        let s = "fn f<'a>(x: &'a str) -> Vec<u8> {\n    let c = 'x'; // note\n    println!(\"hi {}\", 42)\n}";
        assert_eq!(tok("a.rs", s, "fn"), Keyword);
        assert_eq!(tok("a.rs", s, "'a"), Text, "lifetime is not a string");
        assert_eq!(tok("a.rs", s, "'x'"), Str);
        assert_eq!(tok("a.rs", s, "// note"), Comment);
        assert_eq!(tok("a.rs", s, "println"), Func);
        assert_eq!(tok("a.rs", s, "Vec"), Type);
        assert_eq!(tok("a.rs", s, "42"), Number);
    }

    #[test]
    fn typescript_template_strings_span_lines() {
        let s = "@Injectable()\nconst q = `select *\n  from users`;\nif (a < b) return x";
        assert_eq!(tok("a.ts", s, "@Injectable"), Func);
        assert_eq!(tok("a.ts", s, "const"), Keyword);
        assert_eq!(tok("a.ts", s, "from users`"), Str);
        assert_eq!(tok("a.ts", s, "return"), Keyword, "string closed");
        assert_eq!(tok("a.ts", s, "<"), Text, "less-than is not a tag");
    }

    #[test]
    fn react_jsx() {
        let s = "export default function App() {\n  return <Button className=\"big\" onClick={() => go(1)}>\n    Hi\n  </Button>\n}";
        for f in ["a.jsx", "a.tsx", "a.js"] {
            assert_eq!(tok(f, s, "Button"), Type, "{f}");
            assert_eq!(tok(f, s, "className"), Attr);
            assert_eq!(tok(f, s, "\"big\""), Str);
            assert_eq!(tok(f, s, "go"), Func, "JS inside attribute braces");
            assert_eq!(tok(f, s, "</"), Tag);
        }
        let d = "const d = <div>\n  <input\n    disabled\n  />\n</div>";
        assert_eq!(tok("a.jsx", d, "div"), Tag);
        assert_eq!(tok("a.jsx", d, "disabled"), Attr, "multi-line tag");
    }

    #[test]
    fn html_with_inline_script_and_style() {
        let s = "<!DOCTYPE html>\n<a href=\"/x\">A &amp; B</a>\n<!-- note -->\n<script>\n  const x = 1\n</script>\n<style>\n  .card { color: red; }\n</style>\n<p>ok</p>";
        assert_eq!(tok("i.html", s, "<!DOCTYPE html>"), Keyword);
        assert_eq!(tok("i.html", s, "href"), Attr);
        assert_eq!(tok("i.html", s, "\"/x\""), Str);
        assert_eq!(tok("i.html", s, "&amp;"), Number);
        assert_eq!(tok("i.html", s, "<!-- note -->"), Comment);
        assert_eq!(tok("i.html", s, "const"), Keyword);
        assert_eq!(tok("i.html", s, ".card"), Type);
        assert_eq!(tok("i.html", s, "color"), Attr);
        assert_eq!(tok("i.html", s, "<p>"), Tag, "back to markup after </style>");
    }

    #[test]
    fn vue_single_file_component() {
        let s = "<template>\n  <button @click=\"inc\" :disabled=\"busy\">{{ count + 1 }}</button>\n  <input\n    v-model=\"name\"\n  />\n</template>\n<script setup lang=\"ts\">\nimport { ref } from 'vue'\n</script>\n<style scoped>\n.btn { padding: 4px }\n</style>";
        assert_eq!(tok("a.vue", s, "template"), Tag);
        assert_eq!(tok("a.vue", s, "@click"), Keyword);
        assert_eq!(tok("a.vue", s, ":disabled"), Keyword);
        assert_eq!(tok("a.vue", s, "{{"), Keyword);
        assert_eq!(tok("a.vue", s, "1"), Number);
        assert_eq!(tok("a.vue", s, "v-model"), Keyword, "attribute on a later line");
        assert_eq!(tok("a.vue", s, "import"), Keyword);
        assert_eq!(tok("a.vue", s, "'vue'"), Str);
        assert_eq!(tok("a.vue", s, ".btn"), Type);
        assert_eq!(tok("a.vue", s, "padding"), Attr);
        assert_eq!(tok("a.vue", s, "4px"), Number);
    }

    #[test]
    fn svelte_blocks() {
        let s = "<script>\n  let open = false\n</script>\n{#if open}\n  <p on:click={toggle}>{name}</p>\n{/if}";
        assert_eq!(tok("a.svelte", s, "let"), Keyword);
        assert_eq!(tok("a.svelte", s, "#if"), Keyword);
        assert_eq!(tok("a.svelte", s, "/if"), Keyword);
        assert_eq!(tok("a.svelte", s, "on:click"), Keyword);
        assert_eq!(tok("a.svelte", s, "toggle"), Text);
    }

    #[test]
    fn css_and_scss() {
        let s = "@media (max-width: 600px) {\n  a:hover, .card > #main {\n    color: #fff !important;\n    margin: -2px 1.5em;\n    width: var(--w);\n  }\n}\n/* done */";
        assert_eq!(tok("a.css", s, "@media"), Keyword);
        assert_eq!(tok("a.css", s, ":hover"), Func);
        assert_eq!(tok("a.css", s, ".card"), Type);
        assert_eq!(tok("a.css", s, "#main"), Type);
        assert_eq!(tok("a.css", s, "color"), Attr);
        assert_eq!(tok("a.css", s, "#fff"), Number);
        assert_eq!(tok("a.css", s, "!important"), Keyword);
        assert_eq!(tok("a.css", s, "-2px"), Number);
        assert_eq!(tok("a.css", s, "1.5em"), Number);
        assert_eq!(tok("a.css", s, "var"), Func);
        assert_eq!(tok("a.css", s, "/* done */"), Comment);
        let n = ".a {\n  .b { color: red }\n  // note\n}";
        assert_eq!(tok("a.scss", n, ".b"), Type, "nested selector");
        assert_eq!(tok("a.scss", n, "// note"), Comment);
    }

    #[test]
    fn python() {
        let s = "@app.route(\"/\")\ndef home(self):\n    \"\"\"Docs\n    more docs\n    \"\"\"\n    return f\"hi {x}\"  # c";
        assert_eq!(tok("a.py", s, "@app"), Func);
        assert_eq!(tok("a.py", s, "def"), Keyword);
        assert_eq!(tok("a.py", s, "more docs"), Str);
        assert_eq!(tok("a.py", s, "return"), Keyword);
        assert_eq!(tok("a.py", s, "f\"hi"), Str);
        assert_eq!(tok("a.py", s, "# c"), Comment);
    }

    #[test]
    fn sql() {
        let s = "CREATE TABLE users (id uuid, name VARCHAR(50));\nselect * from users where name = 'bob' -- c";
        assert_eq!(tok("a.sql", s, "CREATE"), Keyword);
        assert_eq!(tok("a.sql", s, "uuid"), Keyword);
        assert_eq!(tok("a.sql", s, "VARCHAR"), Keyword);
        assert_eq!(tok("a.sql", s, "select"), Keyword);
        assert_eq!(tok("a.sql", s, "'bob'"), Str);
        assert_eq!(tok("a.sql", s, "-- c"), Comment);
    }

    #[test]
    fn data_and_config_files() {
        let j = "{\n  \"name\": \"nib\",\n  \"ok\": true,\n  \"n\": 12\n}";
        assert_eq!(tok("a.json", j, "\"name\""), Attr);
        assert_eq!(tok("a.json", j, "\"nib\""), Str);
        assert_eq!(tok("a.json", j, "true"), Keyword);
        assert_eq!(tok("a.json", j, "12"), Number);
        let y = "name: nib\nitems:\n  - one\n# c";
        assert_eq!(tok("a.yaml", y, "name"), Attr);
        assert_eq!(tok("a.yaml", y, "-"), Keyword);
        assert_eq!(tok("a.yaml", y, "# c"), Comment);
        let t = "[package]\nversion = \"1.0\"";
        assert_eq!(tok("Cargo.toml", t, "[package]"), Tag);
        assert_eq!(tok("Cargo.toml", t, "version"), Attr);
        assert_eq!(tok(".env", "API_KEY=${HOME}/x", "API_KEY"), Attr);
        assert_eq!(tok(".env", "API_KEY=${HOME}/x", "${HOME}"), Attr);
        assert_eq!(tok("Dockerfile", "FROM node:20\nRUN npm ci", "RUN"), Keyword);
    }

    #[test]
    fn markdown() {
        let s = "# Title\nSome `code` and **bold** and [link](http://x).\n- item\n```js\nconst x = 1\n```\nafter";
        assert_eq!(tok("a.md", s, "# Title"), Keyword);
        assert_eq!(tok("a.md", s, "`code`"), Str);
        assert_eq!(tok("a.md", s, "**bold**"), Type);
        assert_eq!(tok("a.md", s, "[link]"), Func);
        assert_eq!(tok("a.md", s, "(http://x)"), Attr);
        assert_eq!(tok("a.md", s, "-"), Keyword);
        assert_eq!(tok("a.md", s, "const x = 1"), Str);
        assert_eq!(tok("a.md", s, "after"), Text, "fence closed");
    }

    #[test]
    fn go_block_comment_and_raw_string_span_lines() {
        let lines: Vec<String> = ["x /* start", "middle", "end */ y", "s := `raw", "text`", "z"].map(String::from).to_vec();
        let st = line_states(&lines, lang_for(Path::new("a.go")).unwrap());
        let open: Vec<bool> = st.iter().map(|s| s.open != Open::None).collect();
        assert_eq!(open, [false, true, true, false, true, false]);
    }

    #[test]
    fn detects_languages() {
        for (f, name) in [
            ("main.go", "Go"), ("Makefile", "Config"), ("x.tsx", "TSX"), ("x.ts", "TypeScript"), ("x.jsx", "JavaScript"),
            ("a.PY", "Python"), (".env.local", "Config"), ("App.vue", "Vue"), ("x.svelte", "Svelte"),
            ("README.md", "Markdown"), ("s.scss", "SCSS"), ("Dockerfile.dev", "Config"),
        ] {
            assert_eq!(lang_for(Path::new(f)).unwrap().name, name, "{f}");
        }
        assert!(lang_for(Path::new("LICENSE")).is_none());
    }
}
