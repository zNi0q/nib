//! Small, fast syntax highlighter: keywords, strings, comments, numbers,
//! function calls and type names for common languages. Works line by line;
//! the only state carried between lines is "inside a block comment".

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
}

pub struct Lang {
    pub name: &'static str,
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    /// `'` only starts 'x' / '\n' char literals (Rust lifetimes stay plain).
    char_lit: bool,
    kw: &'static [&'static str],
    case_insensitive: bool,
}

const C_KW: &[&str] = &[
    "auto", "bool", "break", "case", "char", "class", "const", "continue", "default", "define", "delete", "do",
    "double", "else", "endif", "enum", "extern", "false", "float", "for", "goto", "if", "ifdef", "ifndef",
    "include", "inline", "int", "long", "namespace", "new", "nullptr", "private", "protected", "public", "return",
    "short", "signed", "sizeof", "static", "struct", "switch", "template", "this", "true", "typedef", "typename",
    "union", "unsigned", "using", "virtual", "void", "volatile", "while", "NULL",
];
const JS_KW: &[&str] = &[
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do",
    "else", "enum", "export", "extends", "false", "finally", "for", "from", "function", "if", "implements",
    "import", "in", "instanceof", "interface", "let", "new", "null", "of", "private", "protected", "public",
    "readonly", "return", "static", "super", "switch", "this", "throw", "true", "try", "type", "typeof",
    "undefined", "var", "void", "while", "yield",
];

static LANGS: &[(&[&str], Lang)] = &[
    (&["rs"], Lang {
        name: "Rust", line: &["//"], block: Some(("/*", "*/")), quotes: &['"'], char_lit: true, case_insensitive: false,
        kw: &["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false", "fn",
              "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
              "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
              "while", "Some", "None", "Ok", "Err"],
    }),
    (&["go"], Lang {
        name: "Go", line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '`'], char_lit: true, case_insensitive: false,
        kw: &["break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "false",
              "for", "func", "go", "goto", "if", "import", "interface", "iota", "map", "nil", "package", "range",
              "return", "select", "struct", "switch", "true", "type", "var"],
    }),
    (&["c", "h", "cpp", "cc", "cxx", "hpp", "hh"], Lang {
        name: "C/C++", line: &["//"], block: Some(("/*", "*/")), quotes: &['"'], char_lit: true, case_insensitive: false, kw: C_KW,
    }),
    (&["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue", "svelte"], Lang {
        name: "JS/TS", line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\'', '`'], char_lit: false, case_insensitive: false, kw: JS_KW,
    }),
    (&["java", "kt", "kts", "cs", "dart", "swift", "scala"], Lang {
        name: "Java-like", line: &["//"], block: Some(("/*", "*/")), quotes: &['"'], char_lit: true, case_insensitive: false,
        kw: &["abstract", "boolean", "break", "case", "catch", "class", "const", "continue", "default", "do",
              "double", "else", "enum", "extends", "false", "final", "finally", "float", "for", "fun", "func",
              "if", "implements", "import", "int", "interface", "let", "long", "namespace", "new", "null",
              "object", "override", "package", "private", "protected", "public", "return", "static", "string",
              "super", "switch", "this", "throw", "throws", "true", "try", "using", "val", "var", "void", "when",
              "while"],
    }),
    (&["py", "pyi"], Lang {
        name: "Python", line: &["#"], block: None, quotes: &['"', '\''], char_lit: false, case_insensitive: false,
        kw: &["and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else",
              "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None",
              "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try", "while", "with", "yield"],
    }),
    (&["zig", "zon"], Lang {
        name: "Zig", line: &["//"], block: None, quotes: &['"'], char_lit: true, case_insensitive: false,
        kw: &["and", "break", "catch", "comptime", "const", "continue", "defer", "else", "enum", "errdefer",
              "error", "export", "extern", "false", "fn", "for", "if", "inline", "null", "or", "orelse", "pub",
              "return", "struct", "switch", "test", "true", "try", "undefined", "union", "unreachable", "var",
              "while"],
    }),
    (&["sh", "bash", "zsh", "fish"], Lang {
        name: "Shell", line: &["#"], block: None, quotes: &['"', '\''], char_lit: false, case_insensitive: false,
        kw: &["case", "do", "done", "echo", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
              "local", "return", "then", "while"],
    }),
    (&["lua"], Lang {
        name: "Lua", line: &["--"], block: Some(("--[[", "]]")), quotes: &['"', '\''], char_lit: false, case_insensitive: false,
        kw: &["and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
              "nil", "not", "or", "repeat", "return", "then", "true", "until", "while"],
    }),
    (&["sql"], Lang {
        name: "SQL", line: &["--"], block: Some(("/*", "*/")), quotes: &['\'', '"'], char_lit: false, case_insensitive: true,
        kw: &["and", "as", "by", "create", "delete", "distinct", "drop", "exists", "foreign", "from", "group",
              "having", "if", "index", "inner", "insert", "into", "join", "key", "left", "limit", "not", "null",
              "on", "or", "order", "primary", "references", "right", "select", "set", "table", "update",
              "values", "where"],
    }),
    (&["css", "scss", "less"], Lang {
        name: "CSS", line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\''], char_lit: false, case_insensitive: false, kw: &["important"],
    }),
    (&["html", "htm", "xml", "svg"], Lang {
        name: "HTML", line: &[], block: Some(("<!--", "-->")), quotes: &['"', '\''], char_lit: false, case_insensitive: false, kw: &[],
    }),
    (&["json", "jsonc"], Lang {
        name: "JSON", line: &["//"], block: None, quotes: &['"'], char_lit: false, case_insensitive: false, kw: &["true", "false", "null"],
    }),
    (&["toml", "yaml", "yml", "ini", "conf", "cfg", "env", "mk", "dockerfile"], Lang {
        name: "Config", line: &["#"], block: None, quotes: &['"', '\''], char_lit: false, case_insensitive: false, kw: &["true", "false"],
    }),
];

pub fn lang_for(path: &Path) -> Option<&'static Lang> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let ext = match name.as_str() {
        "makefile" | "gnumakefile" => "mk",
        "dockerfile" => "dockerfile",
        n if n.starts_with(".env") => "env",
        n if n.starts_with(".bashrc") || n.starts_with(".zshrc") => "sh",
        _ => name.rsplit_once('.')?.1,
    };
    LANGS.iter().find(|(exts, _)| exts.contains(&ext)).map(|(_, l)| l)
}

fn at(cs: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for p in pat.chars() {
        if cs.get(j) != Some(&p) {
            return false;
        }
        j += 1;
    }
    true
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One token kind per char of `line`. `in_block` carries block-comment state
/// from the previous line and is updated for the next one.
pub fn highlight_line(line: &str, lang: &Lang, in_block: &mut bool) -> Vec<Tok> {
    let cs: Vec<char> = line.chars().collect();
    let n = cs.len();
    let mut out = vec![Tok::Text; n];
    let mut i = 0;
    while i < n {
        if *in_block {
            let (_, end) = lang.block.unwrap();
            let stop = (i..n).find(|&j| at(&cs, j, end)).map(|j| j + end.chars().count());
            let stop_at = stop.unwrap_or(n);
            out[i..stop_at].fill(Tok::Comment);
            *in_block = stop.is_none();
            i = stop_at;
            continue;
        }
        if lang.line.iter().any(|p| at(&cs, i, p)) && !lang.block.is_some_and(|(s, _)| at(&cs, i, s)) {
            out[i..].fill(Tok::Comment);
            break;
        }
        if let Some((start, _)) = lang.block {
            if at(&cs, i, start) {
                let len = start.chars().count();
                out[i..i + len].fill(Tok::Comment);
                *in_block = true;
                i += len;
                continue;
            }
        }
        let c = cs[i];
        let quoted = lang.quotes.contains(&c)
            || (c == '\'' && lang.char_lit && (cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&'\'')));
        if quoted {
            let mut j = i + 1;
            while j < n && cs[j] != c {
                j += if cs[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(n);
            out[i..end].fill(Tok::Str);
            i = end;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !ident(cs[i - 1])) {
            let mut j = i;
            while j < n && (ident(cs[j]) || cs[j] == '.') {
                j += 1;
            }
            out[i..j].fill(Tok::Number);
            i = j;
            continue;
        }
        if ident(c) {
            let mut j = i;
            while j < n && ident(cs[j]) {
                j += 1;
            }
            let word: String = cs[i..j].iter().collect();
            let is_kw = if lang.case_insensitive {
                lang.kw.iter().any(|k| k.eq_ignore_ascii_case(&word))
            } else {
                lang.kw.contains(&word.as_str())
            };
            let next = cs[j..].iter().find(|c| **c != ' ');
            let tok = if is_kw {
                Tok::Keyword
            } else if matches!(next, Some('(') | Some('!')) {
                Tok::Func
            } else if c.is_uppercase() {
                Tok::Type
            } else {
                Tok::Text
            };
            out[i..j].fill(tok);
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Block-comment state at the start of each line.
pub fn block_states(lines: &[String], lang: &Lang) -> Vec<bool> {
    let mut state = false;
    let mut out = Vec::with_capacity(lines.len());
    for l in lines {
        out.push(state);
        if lang.block.is_some() {
            highlight_line(l, lang, &mut state);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Tok::*;

    fn toks(path: &str, line: &str) -> String {
        let lang = lang_for(Path::new(path)).unwrap();
        let mut b = false;
        highlight_line(line, lang, &mut b)
            .iter()
            .map(|t| match t {
                Text => '.', Keyword => 'k', Str => 's', Comment => 'c', Number => 'n', Func => 'f', Type => 't',
            })
            .collect()
    }

    #[test]
    fn rust_line() {
        assert_eq!(toks("a.rs", r#"let x = foo("hi", 42); // yo"#),
                                   "kkk.....fff.ssss..nn...ccccc");
    }

    #[test]
    fn rust_lifetime_is_not_a_string_but_char_literal_is() {
        assert_eq!(toks("a.rs", "fn f<'a>(c: 'x')"), "kk..........sss.");
    }

    #[test]
    fn escaped_quotes_stay_inside_string() {
        assert_eq!(toks("a.js", r#"'it\'s' + x"#), "sssssss....");
    }

    #[test]
    fn block_comment_spans_lines() {
        let lang = lang_for(Path::new("a.go")).unwrap();
        let lines: Vec<String> = ["x /* start", "middle", "end */ y", "z"].map(String::from).to_vec();
        assert_eq!(block_states(&lines, lang), [false, true, true, false]);
        let mut st = true;
        let t = highlight_line("end */ y", lang, &mut st);
        assert_eq!(&t[..6], &[Comment; 6]);
        assert_eq!(t[7], Text);
        assert!(!st);
    }

    #[test]
    fn detects_languages() {
        for (f, name) in [("main.go", "Go"), ("Makefile", "Config"), ("x.tsx", "JS/TS"), ("a.PY", "Python"), (".env.local", "Config")] {
            assert_eq!(lang_for(Path::new(f)).unwrap().name, name, "{f}");
        }
        assert!(lang_for(Path::new("README")).is_none());
        assert_eq!(toks("q.sql", "SELECT 1"), "kkkkkk.n");
    }
}
