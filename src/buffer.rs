//! Text buffer: file loading/saving, cursor movement, edits and undo.
//! Positions are (line, char index); display columns are computed separately
//! so tabs and wide characters render correctly.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthChar;

pub const TAB_WIDTH: usize = 4;
const MAX_FILE: u64 = 50 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub y: usize,
    pub x: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Insert,
    Delete,
}

#[derive(Clone, Debug)]
struct Edit {
    kind: Kind,
    at: Pos,
    text: String,
    before: Pos,
    after: Pos,
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub path: Option<PathBuf>,
    pub cur: Pos,
    want_col: usize,
    pub dirty: bool,
    /// Bumped on every change; lets the UI cache per-version work.
    pub version: u64,
    pub crlf: bool,
    trailing_newline: bool,
    /// "\t" for files already indented with tabs, otherwise spaces.
    pub indent: &'static str,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

pub fn byte_idx(s: &str, x: usize) -> usize {
    s.char_indices().nth(x).map_or(s.len(), |(i, _)| i)
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub fn char_width(c: char, col: usize) -> usize {
    match c {
        '\t' => TAB_WIDTH - col % TAB_WIDTH,
        c if c.is_control() => 1,
        c => c.width().unwrap_or(1),
    }
}

/// Display column of char index `x` in `line`.
pub fn display_col(line: &str, x: usize) -> usize {
    line.chars().take(x).fold(0, |col, c| col + char_width(c, col))
}

/// Char index whose cell covers display column `col` (clamped to line end).
pub fn char_at_col(line: &str, col: usize) -> usize {
    let mut c = 0;
    for (i, ch) in line.chars().enumerate() {
        let w = char_width(ch, c);
        if c + w > col {
            return i;
        }
        c += w;
    }
    char_len(line)
}

fn end_of(at: Pos, text: &str) -> Pos {
    let mut parts = text.split('\n');
    let first = parts.next().unwrap_or("");
    let mut end = Pos { y: at.y, x: at.x + char_len(first) };
    for p in parts {
        end = Pos { y: end.y + 1, x: char_len(p) };
    }
    end
}

impl Buffer {
    /// New, empty buffer. `path` is where it will be saved (may not exist yet).
    pub fn empty(path: Option<PathBuf>) -> Self {
        let mut b = Self::from_text("", path);
        b.trailing_newline = true;
        b
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let meta = fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            return Err("is a directory".into());
        }
        if meta.len() > MAX_FILE {
            return Err(format!("file is larger than {} MB", MAX_FILE / 1024 / 1024));
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes[..bytes.len().min(8192)].contains(&0) {
            return Err("binary file".into());
        }
        let text = String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string())?;
        Ok(Self::from_text(&text, Some(path.to_path_buf())))
    }

    pub fn from_text(text: &str, path: Option<PathBuf>) -> Self {
        let crlf = text.contains("\r\n");
        let text = if crlf { text.replace("\r\n", "\n") } else { text.to_string() };
        let trailing_newline = text.ends_with('\n');
        let body = if trailing_newline { &text[..text.len() - 1] } else { &text[..] };
        let lines: Vec<String> = body.split('\n').map(String::from).collect();
        let indent = if lines.iter().any(|l| l.starts_with('\t')) { "\t" } else { "    " };
        Buffer {
            lines,
            path,
            cur: Pos::default(),
            want_col: 0,
            dirty: false,
            version: 0,
            crlf,
            trailing_newline,
            indent,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Full file contents exactly as they will be written to disk.
    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        if self.trailing_newline {
            s.push('\n');
        }
        if self.crlf { s.replace('\n', "\r\n") } else { s }
    }

    /// Save atomically: write a temp file next to the target, then rename it
    /// over the original, so a crash never leaves a half-written file.
    pub fn save(&mut self) -> Result<(), String> {
        let path = self.path.clone().ok_or("no file name")?;
        let target = fs::canonicalize(&path).unwrap_or(path);
        let dir = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = target.file_name().ok_or("invalid file name")?.to_string_lossy();
        let tmp = dir.join(format!(".{name}.nib-tmp"));
        let data = self.text();

        let atomic = (|| -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data.as_bytes())?;
            f.sync_all()?;
            if let Ok(m) = fs::metadata(&target) {
                fs::set_permissions(&tmp, m.permissions())?;
            }
            fs::rename(&tmp, &target)
        })();
        if let Err(e) = atomic {
            let _ = fs::remove_file(&tmp);
            // Directory not writable (but the file may be): write in place.
            if e.kind() != std::io::ErrorKind::PermissionDenied {
                return Err(e.to_string());
            }
            fs::write(&target, data.as_bytes()).map_err(|e| e.to_string())?;
        }
        self.dirty = false;
        Ok(())
    }

    // ---- raw edits (no undo bookkeeping) ----

    fn raw_insert(&mut self, at: Pos, text: &str) -> Pos {
        let bi = byte_idx(&self.lines[at.y], at.x);
        let tail = self.lines[at.y].split_off(bi);
        let mut parts = text.split('\n');
        self.lines[at.y].push_str(parts.next().unwrap_or(""));
        let mut y = at.y;
        for p in parts {
            y += 1;
            self.lines.insert(y, p.to_string());
        }
        self.lines[y].push_str(&tail);
        self.version += 1;
        end_of(at, text)
    }

    fn raw_delete(&mut self, a: Pos, b: Pos) -> String {
        self.version += 1;
        let ab = byte_idx(&self.lines[a.y], a.x);
        let bb = byte_idx(&self.lines[b.y], b.x);
        if a.y == b.y {
            let removed = self.lines[a.y][ab..bb].to_string();
            self.lines[a.y].replace_range(ab..bb, "");
            return removed;
        }
        let mut removed = self.lines[a.y][ab..].to_string();
        for l in &self.lines[a.y + 1..b.y] {
            removed.push('\n');
            removed.push_str(l);
        }
        removed.push('\n');
        removed.push_str(&self.lines[b.y][..bb]);
        let tail = self.lines[b.y][bb..].to_string();
        self.lines[a.y].truncate(ab);
        self.lines[a.y].push_str(&tail);
        self.lines.drain(a.y + 1..=b.y);
        removed
    }

    // ---- edits with undo ----

    fn record(&mut self, e: Edit) {
        self.redo.clear();
        self.dirty = true;
        if let Some(last) = self.undo.last_mut() {
            let single = !e.text.contains('\n') && !last.text.contains('\n');
            // Consecutive typing becomes one undo step.
            if single && e.kind == Kind::Insert && last.kind == Kind::Insert && last.after == e.at {
                last.text.push_str(&e.text);
                last.after = e.after;
                return;
            }
            // Consecutive backspaces too.
            if single && e.kind == Kind::Delete && last.kind == Kind::Delete && end_of(e.at, &e.text) == last.at {
                last.text.insert_str(0, &e.text);
                last.at = e.at;
                last.after = e.after;
                return;
            }
        }
        self.undo.push(e);
    }

    pub fn insert(&mut self, text: &str) {
        let at = self.cur;
        let after = self.raw_insert(at, text);
        self.record(Edit { kind: Kind::Insert, at, text: text.to_string(), before: at, after });
        self.set_cur(after);
    }

    fn delete(&mut self, a: Pos, b: Pos) -> String {
        let before = self.cur;
        let text = self.raw_delete(a, b);
        self.record(Edit { kind: Kind::Delete, at: a, text: text.clone(), before, after: a });
        self.set_cur(a);
        text
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else { return false };
        match e.kind {
            Kind::Insert => { self.raw_delete(e.at, e.after); }
            Kind::Delete => { self.raw_insert(e.at, &e.text); }
        }
        self.set_cur(e.before);
        self.dirty = true;
        self.redo.push(e);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        match e.kind {
            Kind::Insert => { self.raw_insert(e.at, &e.text); }
            Kind::Delete => { self.raw_delete(e.at, end_of(e.at, &e.text)); }
        }
        self.set_cur(e.after);
        self.dirty = true;
        self.undo.push(e);
        true
    }

    // ---- editing commands ----

    pub fn newline(&mut self) {
        let line = &self.lines[self.cur.y];
        let before = &line[..byte_idx(line, self.cur.x)];
        let mut indent: String = before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if before.trim_end().ends_with(['{', '(', '[']) {
            indent.push_str(self.indent);
        }
        self.insert(&format!("\n{indent}"));
    }

    pub fn tab(&mut self) {
        if self.indent == "\t" {
            self.insert("\t");
        } else {
            let col = display_col(&self.lines[self.cur.y], self.cur.x);
            self.insert(&" ".repeat(TAB_WIDTH - col % TAB_WIDTH));
        }
    }

    pub fn backspace(&mut self) {
        let Pos { y, x } = self.cur;
        if x > 0 {
            let line = &self.lines[y];
            let before = &line[..byte_idx(line, x)];
            // In leading spaces, remove back to the previous indent stop.
            let n = if self.indent != "\t" && !before.is_empty() && before.chars().all(|c| c == ' ') {
                (x - 1) % TAB_WIDTH + 1
            } else {
                1
            };
            self.delete(Pos { y, x: x - n }, self.cur);
        } else if y > 0 {
            let prev = char_len(&self.lines[y - 1]);
            self.delete(Pos { y: y - 1, x: prev }, self.cur);
        }
    }

    pub fn delete_forward(&mut self) {
        let Pos { y, x } = self.cur;
        if x < char_len(&self.lines[y]) {
            self.delete(self.cur, Pos { y, x: x + 1 });
        } else if y + 1 < self.lines.len() {
            self.delete(self.cur, Pos { y: y + 1, x: 0 });
        }
    }

    /// Remove the current line and return it (with trailing newline).
    pub fn cut_line(&mut self) -> String {
        let y = self.cur.y;
        let line = self.lines[y].clone() + "\n";
        if y + 1 < self.lines.len() {
            self.delete(Pos { y, x: 0 }, Pos { y: y + 1, x: 0 });
        } else if y > 0 {
            let prev = char_len(&self.lines[y - 1]);
            let end = char_len(&self.lines[y]);
            self.delete(Pos { y: y - 1, x: prev }, Pos { y, x: end });
            self.set_cur(Pos { y: y - 1, x: 0 });
        } else {
            let end = char_len(&self.lines[0]);
            self.delete(Pos { y: 0, x: 0 }, Pos { y: 0, x: end });
        }
        line
    }

    /// Insert whole lines above the current line.
    pub fn paste_lines(&mut self, text: &str) {
        self.set_cur(Pos { y: self.cur.y, x: 0 });
        self.insert(text);
    }

    /// Jump to the next occurrence of `q` after the cursor, wrapping around.
    pub fn find_next(&mut self, q: &str) -> bool {
        if q.is_empty() {
            return false;
        }
        let n = self.lines.len();
        for i in 0..=n {
            let y = (self.cur.y + i) % n;
            let line = &self.lines[y];
            let start = if i == 0 { byte_idx(line, self.cur.x + 1).min(line.len()) } else { 0 };
            let hit = line[start..].find(q).map(|b| start + b);
            let hit = hit.filter(|b| i < n || *b < byte_idx(line, self.cur.x + 1));
            if let Some(b) = hit {
                let x = char_len(&line[..b]);
                self.set_cur(Pos { y, x });
                return true;
            }
        }
        false
    }

    // ---- cursor movement ----

    pub fn set_cur(&mut self, p: Pos) {
        let y = p.y.min(self.lines.len() - 1);
        let x = p.x.min(char_len(&self.lines[y]));
        self.cur = Pos { y, x };
        self.want_col = display_col(&self.lines[y], x);
    }

    fn move_vert(&mut self, y: usize) {
        let y = y.min(self.lines.len() - 1);
        self.cur = Pos { y, x: char_at_col(&self.lines[y], self.want_col) };
    }

    pub fn left(&mut self) {
        let Pos { y, x } = self.cur;
        if x > 0 {
            self.set_cur(Pos { y, x: x - 1 });
        } else if y > 0 {
            self.set_cur(Pos { y: y - 1, x: usize::MAX });
        }
    }

    pub fn right(&mut self) {
        let Pos { y, x } = self.cur;
        if x < char_len(&self.lines[y]) {
            self.set_cur(Pos { y, x: x + 1 });
        } else if y + 1 < self.lines.len() {
            self.set_cur(Pos { y: y + 1, x: 0 });
        }
    }

    pub fn up(&mut self, n: usize) {
        self.move_vert(self.cur.y.saturating_sub(n));
    }

    pub fn down(&mut self, n: usize) {
        self.move_vert(self.cur.y + n);
    }

    /// Toggle between first non-blank character and column 0.
    pub fn home(&mut self) {
        let line = &self.lines[self.cur.y];
        let first = line.chars().take_while(|c| c.is_whitespace()).count();
        let x = if self.cur.x == first { 0 } else { first };
        self.set_cur(Pos { y: self.cur.y, x });
    }

    pub fn end(&mut self) {
        self.set_cur(Pos { y: self.cur.y, x: usize::MAX });
    }

    pub fn doc_start(&mut self) {
        self.set_cur(Pos::default());
    }

    pub fn doc_end(&mut self) {
        self.set_cur(Pos { y: usize::MAX, x: usize::MAX });
    }

    pub fn word_left(&mut self) {
        let Pos { y, x } = self.cur;
        if x == 0 {
            return self.left();
        }
        let cs: Vec<char> = self.lines[y].chars().collect();
        let mut i = x;
        while i > 0 && !is_word(cs[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word(cs[i - 1]) {
            i -= 1;
        }
        self.set_cur(Pos { y, x: i });
    }

    pub fn word_right(&mut self) {
        let Pos { y, x } = self.cur;
        let cs: Vec<char> = self.lines[y].chars().collect();
        if x >= cs.len() {
            return self.right();
        }
        let mut i = x;
        while i < cs.len() && !is_word(cs[i]) {
            i += 1;
        }
        while i < cs.len() && is_word(cs[i]) {
            i += 1;
        }
        self.set_cur(Pos { y, x: i });
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s, None)
    }

    #[test]
    fn roundtrips_line_endings() {
        for s in ["", "a", "a\n", "a\nb\n", "a\r\nb\r\n", "a\n\n", "\n"] {
            assert_eq!(buf(s).text(), s, "input {s:?}");
        }
        assert!(buf("x\r\ny").crlf);
    }

    #[test]
    fn typing_and_multiline_insert_delete() {
        let mut b = buf("hello\nworld");
        b.set_cur(Pos { y: 0, x: 5 });
        b.insert(",\nbig");
        assert_eq!(b.lines, ["hello,", "big", "world"]);
        assert_eq!(b.cur, Pos { y: 1, x: 3 });
        b.set_cur(Pos { y: 1, x: 0 });
        b.backspace();
        assert_eq!(b.lines, ["hello,big", "world"]);
        assert_eq!(b.cur, Pos { y: 0, x: 6 });
        b.end();
        b.delete_forward();
        assert_eq!(b.lines, ["hello,bigworld"]);
        assert!(b.dirty);
    }

    #[test]
    fn undo_redo_groups_typing() {
        let mut b = buf("");
        for c in "abc".chars() {
            b.insert(&c.to_string());
        }
        b.newline();
        b.insert("d");
        assert_eq!(b.text(), "abc\nd");
        b.undo();
        assert_eq!(b.text(), "abc\n");
        b.undo();
        assert_eq!(b.text(), "abc");
        b.undo();
        assert_eq!(b.text(), "");
        assert!(!b.undo());
        b.redo();
        b.redo();
        assert_eq!(b.text(), "abc\n");
        assert_eq!(b.cur, Pos { y: 1, x: 0 });
        b.backspace(); // joins lines
        b.backspace();
        b.backspace();
        assert_eq!(b.text(), "a");
        b.undo();
        assert_eq!(b.text(), "abc", "consecutive backspaces undo as one step");
        b.undo();
        assert_eq!(b.text(), "abc\n", "line join is its own step");
    }

    #[test]
    fn newline_auto_indents() {
        let mut b = buf("    fn x() {");
        b.end();
        b.newline();
        assert_eq!(b.lines[1], "        ");
        let mut t = buf("\tif x {");
        t.end();
        t.newline();
        assert_eq!(t.lines[1], "\t\t");
    }

    #[test]
    fn tab_and_backspace_use_indent_stops() {
        let mut b = buf("ab");
        b.end();
        b.tab();
        assert_eq!(b.lines[0], "ab  ");
        let mut b = buf("");
        b.tab();
        b.tab();
        assert_eq!(b.lines[0], "        ");
        b.backspace();
        assert_eq!(b.lines[0], "    ");
    }

    #[test]
    fn tabs_and_wide_chars_display_columns() {
        assert_eq!(display_col("\tab", 1), 4);
        assert_eq!(display_col("a\tb", 2), 4);
        assert_eq!(display_col("日本", 2), 4);
        assert_eq!(char_at_col("\tab", 5), 2);
        assert_eq!(char_at_col("ab", 10), 2);
    }

    #[test]
    fn vertical_motion_keeps_column() {
        let mut b = buf("long line\nx\nanother");
        b.set_cur(Pos { y: 0, x: 6 });
        b.down(1);
        assert_eq!(b.cur, Pos { y: 1, x: 1 });
        b.down(1);
        assert_eq!(b.cur, Pos { y: 2, x: 6 });
    }

    #[test]
    fn find_wraps_around() {
        let mut b = buf("foo\nbar foo\nbaz");
        assert!(b.find_next("foo"));
        assert_eq!(b.cur, Pos { y: 1, x: 4 });
        assert!(b.find_next("foo"));
        assert_eq!(b.cur, Pos { y: 0, x: 0 });
        assert!(!b.find_next("nope"));
    }

    #[test]
    fn cut_and_paste_line() {
        let mut b = buf("a\nb\nc");
        b.set_cur(Pos { y: 1, x: 0 });
        let l = b.cut_line();
        assert_eq!((l.as_str(), b.text().as_str()), ("b\n", "a\nc"));
        b.set_cur(Pos { y: 0, x: 0 });
        b.paste_lines(&l);
        assert_eq!(b.text(), "b\na\nc");
        let mut last = buf("a\nz");
        last.set_cur(Pos { y: 1, x: 0 });
        assert_eq!(last.cut_line(), "z\n");
        assert_eq!(last.text(), "a");
    }

    #[test]
    fn word_motion() {
        let mut b = buf("let foo_bar = 1;");
        b.word_right();
        assert_eq!(b.cur.x, 3);
        b.word_right();
        assert_eq!(b.cur.x, 11);
        b.word_left();
        assert_eq!(b.cur.x, 4);
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nib-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_is_atomic_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp_dir("save");
        let p = d.join("run.sh");
        fs::write(&p, "echo hi\r\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let mut b = Buffer::open(&p).unwrap();
        b.end();
        b.insert(" there");
        b.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "echo hi there\r\n");
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!b.dirty);
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "temp file left behind");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn refuses_binary_and_non_utf8() {
        let d = tmp_dir("bin");
        fs::write(d.join("a.bin"), [0u8, 1, 2]).unwrap();
        fs::write(d.join("b.txt"), [0xffu8, 0xfe, b'a']).unwrap();
        assert_eq!(Buffer::open(&d.join("a.bin")).err().unwrap(), "binary file");
        assert_eq!(Buffer::open(&d.join("b.txt")).err().unwrap(), "not UTF-8 text");
        assert!(Buffer::open(&d).is_err());
        fs::remove_dir_all(d).unwrap();
    }
}
