use std::io::{self, Write};

use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Reset,
    Black,
    Rgb(u8, u8, u8),
}

const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const REVERSED: u8 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    mods: u8,
}

impl Style {
    pub fn fg(mut self, c: Color) -> Style {
        self.fg = c;
        self
    }

    pub fn bg(mut self, c: Color) -> Style {
        self.bg = c;
        self
    }

    pub fn bold(mut self) -> Style {
        self.mods |= BOLD;
        self
    }

    pub fn italic(mut self) -> Style {
        self.mods |= ITALIC;
        self
    }

    pub fn reversed(mut self) -> Style {
        self.mods |= REVERSED;
        self
    }

    pub fn is_bold(&self) -> bool {
        self.mods & BOLD != 0
    }
}

pub fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersection(&self, o: Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect { x, y, width: r.saturating_sub(x), height: b.saturating_sub(y) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    ch: char,
    style: u16,
}

const BLANK: Cell = Cell { ch: ' ', style: 0 };
const WIDE_TAIL: char = '\0';
const UNKNOWN: Cell = Cell { ch: '\u{fffe}', style: u16::MAX };
const MAX_STYLES: usize = 4096;

pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    shown: Vec<Cell>,
    styles: Vec<Style>,
    cursor: Option<(u16, u16)>,
    out: Vec<u8>,
}

impl Screen {
    pub fn new(width: u16, height: u16) -> Screen {
        let n = width as usize * height as usize;
        Screen {
            width,
            height,
            cells: vec![BLANK; n],
            shown: vec![UNKNOWN; n],
            styles: vec![Style::default()],
            cursor: None,
            out: Vec::new(),
        }
    }

    pub fn area(&self) -> Rect {
        Rect { x: 0, y: 0, width: self.width, height: self.height }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        if (width, height) != (self.width, self.height) {
            *self = Screen::new(width, height);
        }
    }

    pub fn begin(&mut self) {
        self.cells.fill(BLANK);
        self.cursor = None;
        if self.styles.len() > MAX_STYLES {
            self.styles.truncate(1);
            self.shown.fill(UNKNOWN);
        }
    }

    fn style_id(&mut self, s: Style) -> u16 {
        match self.styles.iter().position(|x| *x == s) {
            Some(i) => i as u16,
            None => {
                self.styles.push(s);
                (self.styles.len() - 1) as u16
            }
        }
    }

    fn cell(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        if x < self.width && y < self.height {
            Some(&mut self.cells[y as usize * self.width as usize + x as usize])
        } else {
            None
        }
    }

    pub fn fill(&mut self, r: Rect, style: Style) {
        let id = self.style_id(style);
        let r = r.intersection(self.area());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                *self.cell(x, y).unwrap() = Cell { ch: ' ', style: id };
            }
        }
    }

    pub fn clear(&mut self, r: Rect) {
        self.fill(r, Style::default());
    }

    pub fn set_cursor(&mut self, x: u16, y: u16) {
        self.cursor = Some((x, y));
    }

    pub fn put(&mut self, x: u16, y: u16, text: &str, style: Style, max_x: u16) -> u16 {
        let id = self.style_id(style);
        let max_x = max_x.min(self.width);
        let mut x = x;
        for ch in text.chars() {
            let w = ch.width().unwrap_or(0) as u16;
            if w == 0 {
                continue;
            }
            if x + w > max_x {
                if x < max_x {
                    if let Some(c) = self.cell(x, y) {
                        *c = Cell { ch: ' ', style: id };
                    }
                }
                return max_x;
            }
            if let Some(c) = self.cell(x, y) {
                *c = Cell { ch, style: id };
            }
            if w == 2 {
                if let Some(c) = self.cell(x + 1, y) {
                    *c = Cell { ch: WIDE_TAIL, style: id };
                }
            }
            x += w;
        }
        x
    }

    pub fn put_spans(&mut self, x: u16, y: u16, spans: &[(&str, Style)], max_x: u16) -> u16 {
        spans.iter().fold(x, |x, (t, s)| self.put(x, y, t, *s, max_x))
    }

    pub fn boxed(&mut self, r: Rect, border: Style, title: &str, title_style: Style) {
        if r.width < 2 || r.height < 2 {
            return;
        }
        let (right, bottom) = (r.right() - 1, r.bottom() - 1);
        self.clear(r);
        let line = "─".repeat(r.width as usize - 2);
        self.put(r.x, r.y, "┌", border, r.right());
        self.put(r.x + 1, r.y, &line, border, right);
        self.put(right, r.y, "┐", border, r.right());
        self.put(r.x, bottom, "└", border, r.right());
        self.put(r.x + 1, bottom, &line, border, right);
        self.put(right, bottom, "┘", border, r.right());
        for y in r.y + 1..bottom {
            self.put(r.x, y, "│", border, r.right());
            self.put(right, y, "│", border, r.right());
        }
        self.put(r.x + 1, r.y, title, title_style, right);
    }

    pub fn flush(&mut self, w: &mut impl Write) -> io::Result<()> {
        let out = &mut self.out;
        out.clear();
        out.extend_from_slice(b"\x1b[?2026h\x1b[?25l");
        let mut pos: Option<(u16, u16)> = None;
        let mut last_style: Option<u16> = None;
        let width = self.width as usize;
        for (i, cell) in self.cells.iter().enumerate() {
            if cell.ch == WIDE_TAIL || *cell == self.shown[i] {
                continue;
            }
            let (x, y) = ((i % width) as u16, (i / width) as u16);
            if pos != Some((x, y)) {
                write!(out, "\x1b[{};{}H", y + 1, x + 1)?;
            }
            if last_style != Some(cell.style) {
                sgr(out, &self.styles[cell.style as usize]);
                last_style = Some(cell.style);
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(cell.ch.encode_utf8(&mut buf).as_bytes());
            pos = Some((x + cell.ch.width().unwrap_or(1).max(1) as u16, y));
        }
        out.extend_from_slice(b"\x1b[0m");
        if let Some((x, y)) = self.cursor {
            write!(out, "\x1b[{};{}H\x1b[?25h", y + 1, x + 1)?;
        }
        out.extend_from_slice(b"\x1b[?2026l");
        w.write_all(out)?;
        w.flush()?;
        self.shown.copy_from_slice(&self.cells);
        if self.out.capacity() > 64 * 1024 {
            self.out = Vec::new();
        }
        Ok(())
    }

    pub fn row_text(&self, y: u16) -> String {
        let start = y as usize * self.width as usize;
        self.cells[start..start + self.width as usize].iter().filter(|c| c.ch != WIDE_TAIL).map(|c| c.ch).collect()
    }
}

fn sgr(out: &mut Vec<u8>, s: &Style) {
    out.extend_from_slice(b"\x1b[0");
    if s.mods & BOLD != 0 {
        out.extend_from_slice(b";1");
    }
    if s.mods & ITALIC != 0 {
        out.extend_from_slice(b";3");
    }
    if s.mods & REVERSED != 0 {
        out.extend_from_slice(b";7");
    }
    let mut color = |c: Color, base: u8| match c {
        Color::Reset => {}
        Color::Black => {
            let _ = write!(out, ";{base}0");
        }
        Color::Rgb(r, g, b) => {
            let _ = write!(out, ";{base}8;2;{r};{g};{b}");
        }
    };
    color(s.fg, 3);
    color(s.bg, 4);
    out.push(b'm');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_tiny() {
        assert_eq!(std::mem::size_of::<Cell>(), 8);
    }

    #[test]
    fn put_clips_and_handles_wide_chars() {
        let mut s = Screen::new(6, 2);
        assert_eq!(s.put(0, 0, "ab日本語", Style::default(), 6), 6);
        assert_eq!(s.row_text(0), "ab日本");
        assert_eq!(s.put(0, 1, "abcde日", Style::default(), 6), 6);
        assert_eq!(s.row_text(1), "abcde ", "wide char that doesn't fit becomes a space");
        s.put(10, 0, "x", Style::default(), 20);
        assert_eq!(s.row_text(0), "ab日本", "off screen is ignored");
    }

    #[test]
    fn flush_sends_only_changes() {
        let red = fg(Color::Rgb(255, 0, 0));
        let mut s = Screen::new(10, 2);
        s.put(0, 0, "hello", red, 10);
        let mut first = Vec::new();
        s.flush(&mut first).unwrap();
        let first = String::from_utf8(first).unwrap();
        assert!(first.contains("\x1b[1;1H") && first.contains("38;2;255;0;0") && first.contains("hello"));

        s.begin();
        s.put(0, 0, "help", red, 10);
        let mut second = Vec::new();
        s.flush(&mut second).unwrap();
        let second = String::from_utf8(second).unwrap();
        assert!(second.contains("\x1b[1;4H") && second.contains("mp"), "{second:?}");
        assert!(!second.contains("hel"), "unchanged cells were resent: {second:?}");

        s.begin();
        s.put(0, 0, "help", red, 10);
        let mut third = Vec::new();
        s.flush(&mut third).unwrap();
        assert_eq!(String::from_utf8(third).unwrap(), "\x1b[?2026h\x1b[?25l\x1b[0m\x1b[?2026l", "identical frame sends no cells");
    }

    #[test]
    fn styles_and_cursor() {
        let mut s = Screen::new(4, 1);
        s.put(0, 0, "x", Style::default().bold().italic().reversed().fg(Color::Black).bg(Color::Rgb(1, 2, 3)), 4);
        s.set_cursor(2, 0);
        let mut out = Vec::new();
        s.flush(&mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("\x1b[0;1;3;7;30;48;2;1;2;3mx"), "{out:?}");
        assert!(out.ends_with("\x1b[1;3H\x1b[?25h\x1b[?2026l"), "{out:?}");
    }

    #[test]
    fn boxes_and_resize() {
        let mut s = Screen::new(8, 3);
        s.boxed(Rect { x: 0, y: 0, width: 8, height: 3 }, Style::default(), " Hi ", Style::default());
        assert_eq!(s.row_text(0), "┌ Hi ──┐");
        assert_eq!(s.row_text(1), "│      │");
        assert_eq!(s.row_text(2), "└──────┘");
        s.resize(3, 1);
        assert_eq!(s.area(), Rect { x: 0, y: 0, width: 3, height: 1 });
    }
}
