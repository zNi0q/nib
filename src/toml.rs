use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<Value>),
    Table(Table),
}

pub type Table = Vec<(String, Value)>;

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn get<'a>(t: &'a Table, key: &str) -> Option<&'a Value> {
    t.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn get_mut<'a>(t: &'a mut Table, key: &str) -> Option<&'a mut Value> {
    t.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Value::String(_))
    }
}

pub fn to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Integer(n) => serde_json::Value::from(*n),
        Value::Float(x) => serde_json::Number::from_f64(*x).map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Array(a) => serde_json::Value::Array(a.iter().map(to_json).collect()),
        Value::Table(t) => serde_json::Value::Object(t.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()),
    }
}

pub fn parse(text: &str) -> Result<Table, Error> {
    Parser { src: text.chars().collect(), pos: 0 }.document()
}

struct Parser {
    src: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.src.get(self.pos + offset).copied()
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(i, c)| self.peek_at(i) == Some(c))
    }

    fn line(&self) -> usize {
        self.src[..self.pos.min(self.src.len())].iter().filter(|c| **c == '\n').count() + 1
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        Err(Error { line: self.line(), message: message.into() })
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while !matches!(self.peek(), None | Some('\n')) {
                self.pos += 1;
            }
        }
    }

    fn skip_blank(&mut self) {
        loop {
            self.skip_spaces();
            self.skip_comment();
            match self.peek() {
                Some('\n') => self.pos += 1,
                Some('\r') if self.peek_at(1) == Some('\n') => self.pos += 2,
                _ => return,
            }
        }
    }

    fn end_of_line(&mut self) -> Result<(), Error> {
        self.skip_spaces();
        self.skip_comment();
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.pos += 1;
                Ok(())
            }
            Some('\r') if self.peek_at(1) == Some('\n') => {
                self.pos += 2;
                Ok(())
            }
            Some(c) => self.error(format!("expected a newline, found {c:?}")),
        }
    }

    fn document(mut self) -> Result<Table, Error> {
        let mut root = Table::new();
        let mut current: Vec<String> = Vec::new();
        let mut headers: Vec<Vec<String>> = Vec::new();
        loop {
            self.skip_blank();
            let Some(c) = self.peek() else { return Ok(root) };
            if c == '[' {
                let line = self.line();
                if self.peek_at(1) == Some('[') {
                    return self.error("arrays of tables ([[...]]) are not supported");
                }
                self.pos += 1;
                self.skip_spaces();
                let path = self.dotted_key()?;
                self.skip_spaces();
                if self.peek() != Some(']') {
                    return self.error("expected `]` to close the table header");
                }
                self.pos += 1;
                self.end_of_line()?;
                if headers.contains(&path) {
                    return Err(Error { line, message: format!("duplicate table [{}]", path.join(".")) });
                }
                table_at(&mut root, &path).map_err(|message| Error { line, message })?;
                headers.push(path.clone());
                current = path;
            } else {
                let line = self.line();
                let key = self.dotted_key()?;
                self.skip_spaces();
                if self.peek() != Some('=') {
                    return self.error("expected `=` after the key");
                }
                self.pos += 1;
                self.skip_spaces();
                let value = self.value()?;
                self.end_of_line()?;
                let table = table_at(&mut root, &current).map_err(|message| Error { line, message })?;
                insert(table, &key, value).map_err(|message| Error { line, message })?;
            }
        }
    }

    fn dotted_key(&mut self) -> Result<Vec<String>, Error> {
        let mut parts = vec![self.simple_key()?];
        loop {
            self.skip_spaces();
            if self.peek() != Some('.') {
                return Ok(parts);
            }
            self.pos += 1;
            self.skip_spaces();
            parts.push(self.simple_key()?);
        }
    }

    fn simple_key(&mut self) -> Result<String, Error> {
        match self.peek() {
            Some('"') => {
                self.pos += 1;
                self.basic_string()
            }
            Some('\'') => {
                self.pos += 1;
                self.literal_string()
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    self.pos += 1;
                }
                if start == self.pos {
                    return match self.peek() {
                        Some(c) => self.error(format!("invalid key, found {c:?}")),
                        None => self.error("expected a key"),
                    };
                }
                Ok(self.src[start..self.pos].iter().collect())
            }
        }
    }

    fn value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some('"') if self.starts_with("\"\"\"") => {
                self.pos += 3;
                self.multiline_basic().map(Value::String)
            }
            Some('"') => {
                self.pos += 1;
                self.basic_string().map(Value::String)
            }
            Some('\'') if self.starts_with("'''") => {
                self.pos += 3;
                self.multiline_literal().map(Value::String)
            }
            Some('\'') => {
                self.pos += 1;
                self.literal_string().map(Value::String)
            }
            Some('[') => {
                self.pos += 1;
                self.array()
            }
            Some('{') => {
                self.pos += 1;
                self.inline_table()
            }
            Some('t') if self.word("true") => Ok(Value::Boolean(true)),
            Some('f') if self.word("false") => Ok(Value::Boolean(false)),
            Some(c) if c.is_ascii_digit() || c == '+' || c == '-' => self.number(),
            Some(c) => self.error(format!("invalid value starting with {c:?}")),
            None => self.error("expected a value"),
        }
    }

    fn word(&mut self, w: &str) -> bool {
        let n = w.chars().count();
        let ends = !self.peek_at(n).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if self.starts_with(w) && ends {
            self.pos += n;
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_' | '.')) {
            self.pos += 1;
        }
        let raw: String = self.src[start..self.pos].iter().collect();
        let digits = raw.replace('_', "");
        let float_like = digits.contains('.') || (digits.contains(['e', 'E']) && !digits.starts_with("0x"));
        let parsed = if float_like {
            digits.parse::<f64>().ok().filter(|x| x.is_finite()).map(Value::Float)
        } else {
            digits.parse::<i64>().ok().map(Value::Integer)
        };
        match parsed {
            Some(v) if !raw.starts_with('_') && !raw.ends_with('_') && !raw.contains("__") => Ok(v),
            _ => {
                self.pos = start;
                self.error(format!("invalid number {raw:?}"))
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), Error> {
        let Some(c) = self.peek() else { return self.error("unfinished escape sequence") };
        self.pos += 1;
        match c {
            'b' => out.push('\u{8}'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'f' => out.push('\u{c}'),
            'r' => out.push('\r'),
            'e' => out.push('\u{1b}'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'u' | 'U' => {
                let len = if c == 'u' { 4 } else { 8 };
                let hex: String = (0..len).filter_map(|i| self.peek_at(i)).collect();
                let ch = (hex.len() == len)
                    .then(|| u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32))
                    .flatten();
                match ch {
                    Some(ch) => {
                        out.push(ch);
                        self.pos += len;
                    }
                    None => return self.error(format!("invalid unicode escape \\{c}{hex}")),
                }
            }
            other => return self.error(format!("invalid escape sequence \\{other}")),
        }
        Ok(())
    }

    fn basic_string(&mut self) -> Result<String, Error> {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => return self.error("unterminated string"),
                Some('"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn literal_string(&mut self) -> Result<String, Error> {
        let start = self.pos;
        loop {
            match self.peek() {
                None | Some('\n') => return self.error("unterminated string"),
                Some('\'') => {
                    let s = self.src[start..self.pos].iter().collect();
                    self.pos += 1;
                    return Ok(s);
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn skip_first_newline(&mut self) {
        if self.peek() == Some('\n') {
            self.pos += 1;
        } else if self.starts_with("\r\n") {
            self.pos += 2;
        }
    }

    fn multiline_basic(&mut self) -> Result<String, Error> {
        self.skip_first_newline();
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return self.error("unterminated multi-line string"),
                Some('"') if self.starts_with("\"\"\"") && !self.starts_with("\"\"\"\"") => {
                    self.pos += 3;
                    return Ok(out);
                }
                Some('\\') => {
                    self.pos += 1;
                    if matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                            self.pos += 1;
                        }
                    } else {
                        self.escape(&mut out)?;
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn multiline_literal(&mut self) -> Result<String, Error> {
        self.skip_first_newline();
        let start = self.pos;
        loop {
            match self.peek() {
                None => return self.error("unterminated multi-line string"),
                Some('\'') if self.starts_with("'''") && !self.starts_with("''''") => {
                    let s = self.src[start..self.pos].iter().collect();
                    self.pos += 3;
                    return Ok(s);
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn array(&mut self) -> Result<Value, Error> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
            self.skip_blank();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                Some(c) => return self.error(format!("expected `,` or `]` in array, found {c:?}")),
                None => return self.error("unterminated array"),
            }
        }
    }

    fn inline_table(&mut self) -> Result<Value, Error> {
        let mut table = Table::new();
        self.skip_blank();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::Table(table));
        }
        loop {
            self.skip_blank();
            let line = self.line();
            let key = self.dotted_key()?;
            self.skip_spaces();
            if self.peek() != Some('=') {
                return self.error("expected `=` after the key");
            }
            self.pos += 1;
            self.skip_spaces();
            let value = self.value()?;
            insert(&mut table, &key, value).map_err(|message| Error { line, message })?;
            self.skip_blank();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Value::Table(table));
                }
                Some(c) => return self.error(format!("expected `,` or `}}` in inline table, found {c:?}")),
                None => return self.error("unterminated inline table"),
            }
        }
    }
}

fn table_at<'a>(root: &'a mut Table, path: &[String]) -> Result<&'a mut Table, String> {
    let mut t = root;
    for (i, part) in path.iter().enumerate() {
        if get(t, part).is_none() {
            t.push((part.clone(), Value::Table(Table::new())));
        }
        match get_mut(t, part) {
            Some(Value::Table(inner)) => t = inner,
            _ => return Err(format!("`{}` is not a table", path[..=i].join("."))),
        }
    }
    Ok(t)
}

fn insert(table: &mut Table, key: &[String], value: Value) -> Result<(), String> {
    let (last, parents) = key.split_last().expect("keys are never empty");
    let t = table_at(table, parents)?;
    if get(t, last).is_some() {
        return Err(format!("duplicate key `{}`", key.join(".")));
    }
    t.push((last.clone(), value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    #[test]
    fn scalars_and_comments() {
        let t = parse("# top\na = 1 # trailing\nb = -2_000\nc = 1.5\nd = true\ne = false\nf = \"x\"\ng = 'y'\n").unwrap();
        assert_eq!(get(&t, "a"), Some(&Value::Integer(1)));
        assert_eq!(get(&t, "b"), Some(&Value::Integer(-2000)));
        assert_eq!(get(&t, "c"), Some(&Value::Float(1.5)));
        assert_eq!(get(&t, "d"), Some(&Value::Boolean(true)));
        assert_eq!(get(&t, "e"), Some(&Value::Boolean(false)));
        assert_eq!(get(&t, "f"), Some(&s("x")));
        assert_eq!(get(&t, "g"), Some(&s("y")));
        assert_eq!(t.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d", "e", "f", "g"], "file order kept");
    }

    #[test]
    fn strings() {
        let t = parse(r#"a = "q\"uote\\ \t\n\u00e9\U0001F600"
b = 'C:\path\#'
c = """
line1
line2\
   joined"""
d = '''
raw \n'''
e = "has # hash"
"#)
        .unwrap();
        assert_eq!(get(&t, "a"), Some(&s("q\"uote\\ \t\né😀")));
        assert_eq!(get(&t, "b"), Some(&s("C:\\path\\#")));
        assert_eq!(get(&t, "c"), Some(&s("line1\nline2joined")));
        assert_eq!(get(&t, "d"), Some(&s("raw \\n")));
        assert_eq!(get(&t, "e"), Some(&s("has # hash")));
    }

    #[test]
    fn tables_arrays_inline_and_dotted_keys() {
        let t = parse(
            "[editor]\ntab = 2\n[a.b]\nx = 1\n[keys]\ndef = [\"f12\",\n  \"ctrl+g\", # c\n]\nnone = []\n[p]\nids = { ts = \"typescript\", n = { deep = 'y' } }\nsite.name = \"n\"\n\"quoted key\" = 1\n",
        )
        .unwrap();
        let editor = get(&t, "editor").and_then(Value::as_table).unwrap();
        assert_eq!(get(editor, "tab").and_then(Value::as_integer), Some(2));
        let b = get(get(&t, "a").unwrap().as_table().unwrap(), "b").unwrap().as_table().unwrap();
        assert_eq!(get(b, "x"), Some(&Value::Integer(1)));
        let keys = get(&t, "keys").and_then(Value::as_table).unwrap();
        assert_eq!(get(keys, "def"), Some(&Value::Array(vec![s("f12"), s("ctrl+g")])));
        assert_eq!(get(keys, "none"), Some(&Value::Array(vec![])));
        let p = get(&t, "p").and_then(Value::as_table).unwrap();
        let ids = get(p, "ids").and_then(Value::as_table).unwrap();
        assert_eq!(get(ids, "ts"), Some(&s("typescript")));
        assert_eq!(get(get(ids, "n").unwrap().as_table().unwrap(), "deep"), Some(&s("y")));
        assert_eq!(get(get(p, "site").unwrap().as_table().unwrap(), "name"), Some(&s("n")));
        assert_eq!(get(p, "quoted key"), Some(&Value::Integer(1)));
    }

    #[test]
    fn errors_have_line_numbers() {
        let cases = [
            ("a = 1\nb = tru\n", 2, "invalid value"),
            ("a = 1\na = 2\n", 2, "duplicate key `a`"),
            ("[x]\n[x]\n", 2, "duplicate table [x]"),
            ("\n\na = \"open\n", 3, "unterminated string"),
            ("a = [1, 2\n", 2, "unterminated array"),
            ("a = 1 2\n", 1, "expected a newline"),
            ("a 1\n", 1, "expected `=`"),
            ("a = 12x\n", 1, "invalid number"),
            ("a = 1\n[[t]]\n", 2, "arrays of tables"),
            ("a = 1\n[a]\n", 2, "`a` is not a table"),
            ("x = { a = 1, a = 2 }\n", 1, "duplicate key `a`"),
            ("a = \"\\q\"\n", 1, "invalid escape"),
            ("a = \"\"\"\nnever closed\n", 3, "unterminated multi-line string"),
            ("= 1\n", 1, "invalid key"),
        ];
        for (src, line, msg) in cases {
            let e = parse(src).unwrap_err();
            assert_eq!(e.line, line, "{src:?}: {e}");
            assert!(e.message.contains(msg), "{src:?}: {e}");
        }
    }

    #[test]
    fn crlf_and_json_conversion() {
        let t = parse("a = 1\r\n[b]\r\nc = [true, 2.5, \"s\", { d = 'e' }]\r\n").unwrap();
        let j = to_json(&Value::Table(t));
        assert_eq!(j, serde_json::json!({"a": 1, "b": {"c": [true, 2.5, "s", {"d": "e"}]}}));
    }

    #[test]
    fn parses_nibs_own_files() {
        parse(&crate::config::template()).unwrap();
        for (name, body) in crate::plugin::presets() {
            parse(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
