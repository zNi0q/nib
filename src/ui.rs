//! Drawing. Colors are a Tokyo Night–style palette on the terminal's own
//! background, so nib blends into whatever theme the terminal uses.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{severity_label, After, App, Focus, Popup, Prompt};
use crate::lsp::{Diag, Severity};
use crate::buffer::{char_width, display_col};
use crate::highlight::{highlight_line, lang_for, Tok};
use crate::icons::{self, icon_for};

const FG: Color = Color::Rgb(192, 202, 245);
const DIM: Color = Color::Rgb(86, 95, 137);
const ACCENT: Color = Color::Rgb(122, 162, 247);
const BAR_BG: Color = Color::Rgb(36, 40, 59);
const SEL_BG: Color = Color::Rgb(41, 46, 66);
const WARN: Color = Color::Rgb(224, 175, 104);
const ERR: Color = Color::Rgb(247, 118, 142);

fn sev_color(s: Severity) -> Color {
    match s {
        Severity::Error => ERR,
        Severity::Warning => WARN,
        _ => ACCENT,
    }
}

fn sev_mark(s: Severity) -> &'static str {
    match s {
        Severity::Error => "●",
        Severity::Warning => "▲",
        _ => "·",
    }
}

/// Diagnostics of the open file.
fn diags(app: &App) -> &[Diag] {
    match (&app.lsp, app.buf.as_ref().and_then(|b| b.path.as_ref())) {
        (Some(l), Some(p)) => l.diags_for(p),
        _ => &[],
    }
}

fn tok_style(t: Tok) -> Style {
    let s = Style::default();
    match t {
        Tok::Text => s.fg(FG),
        Tok::Keyword => s.fg(Color::Rgb(187, 154, 247)),
        Tok::Str => s.fg(Color::Rgb(158, 206, 106)),
        Tok::Comment => s.fg(DIM).add_modifier(Modifier::ITALIC),
        Tok::Number => s.fg(Color::Rgb(255, 158, 100)),
        Tok::Func => s.fg(ACCENT),
        Tok::Type => s.fg(Color::Rgb(42, 195, 222)),
        Tok::Tag => s.fg(Color::Rgb(247, 118, 142)),
        Tok::Attr => s.fg(Color::Rgb(224, 175, 104)),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [main, status, bottom] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(f.area());
    let editor = if app.show_tree {
        let [tree, editor] = Layout::horizontal([Constraint::Length(32), Constraint::Min(10)]).areas(main);
        draw_tree(f, app, tree);
        editor
    } else {
        app.tree_area = Rect::default();
        main
    };
    draw_editor(f, app, editor);
    draw_status(f, app, status);
    draw_bottom(f, app, bottom);
    if let Some(Prompt::Palette { query, sel }) = &app.prompt {
        draw_palette(f, query, *sel);
    } else {
        draw_popup(f, app);
    }
}

fn draw_palette(f: &mut Frame, query: &str, sel: usize) {
    let matches = App::palette_matches(query);
    let screen = f.area();
    let w = 56.min(screen.width.saturating_sub(4));
    let h = (matches.len().max(1) as u16 + 4).min(screen.height.saturating_sub(2));
    let area = Rect { x: screen.x + (screen.width - w) / 2, y: screen.y + screen.height / 6, width: w, height: h };
    let block = Block::bordered()
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" Commands ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let input = Line::from(vec![
        Span::styled(" > ", Style::default().fg(WARN)),
        Span::styled(query.to_string(), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
    ]);
    let mut lines = vec![input, Line::from(Span::styled("─".repeat(inner.width as usize), Style::default().fg(DIM)))];
    if matches.is_empty() {
        lines.push(Line::from(Span::styled(" no matching command", Style::default().fg(DIM))));
    }
    let rows = inner.height.saturating_sub(2) as usize;
    let first = sel.saturating_sub(rows.saturating_sub(1));
    for (i, c) in matches.iter().enumerate().skip(first).take(rows) {
        let keys = c.shortcut();
        let pad = (inner.width as usize).saturating_sub(c.name.chars().count() + keys.len() + 3);
        let mut style = Style::default().fg(FG);
        if i == sel {
            style = style.bg(SEL_BG).fg(ACCENT).add_modifier(Modifier::BOLD);
        }
        lines.push(Line::from(vec![
            Span::styled(format!(" {}{}", c.name, " ".repeat(pad)), style),
            Span::styled(format!("{keys}  "), style.fg(DIM)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
    f.set_cursor_position((inner.x + 3 + query.chars().count() as u16, inner.y));
}

fn draw_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Tree;
    let root = app.tree.root.file_name().map_or("/".into(), |n| n.to_string_lossy().into_owned());
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(if focused { ACCENT } else { DIM }))
        .title(Span::styled(format!(" {root} "), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Leave the first row for the title.
    let list = Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner };
    app.tree_area = list;
    app.follow_cursor();

    let open = app.buf.as_ref().and_then(|b| b.path.clone());
    let show_icons = icons::enabled();
    let lines: Vec<Line> = app
        .tree
        .items
        .iter()
        .enumerate()
        .skip(app.tree.scroll)
        .take(list.height as usize)
        .map(|(i, e)| {
            let (chevron, name, mut style) = if e.is_dir {
                (if e.expanded { "▾ " } else { "▸ " }, format!("{}/", e.name), Style::default().fg(ACCENT))
            } else {
                let s = if open.as_ref() == Some(&e.path) { Style::default().fg(WARN) } else { Style::default().fg(FG) };
                ("  ", e.name.clone(), s)
            };
            let mut icon_style = style;
            let icon = if show_icons {
                let (glyph, color) = icon_for(&e.name, e.is_dir, e.expanded);
                icon_style = icon_style.fg(color);
                format!("{glyph} ")
            } else {
                String::new()
            };
            if i == app.tree.sel {
                style = style.bg(SEL_BG).add_modifier(Modifier::BOLD);
                icon_style = icon_style.bg(SEL_BG);
                if focused {
                    style = style.add_modifier(Modifier::REVERSED);
                    icon_style = icon_style.add_modifier(Modifier::REVERSED);
                }
            }
            let lead = format!(" {}{chevron}", "  ".repeat(e.depth));
            let used = lead.chars().count() + icon.chars().count() + name.chars().count();
            let pad = " ".repeat((list.width as usize).saturating_sub(used));
            Line::from(vec![
                Span::styled(lead, style),
                Span::styled(icon, icon_style),
                Span::styled(format!("{name}{pad}"), style),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
}

fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    if app.buf.is_none() {
        app.text_area = area;
        return draw_welcome(f, area);
    }
    let digits = app.buf.as_ref().unwrap().lines.len().to_string().len().max(3);
    let gutter = digits as u16 + 2;
    let text = Rect { x: area.x + gutter, width: area.width.saturating_sub(gutter), ..area };
    app.text_area = text;
    app.follow_cursor();
    let states = app.line_states().to_vec();
    let buf = app.buf.as_ref().unwrap();
    let lang = buf.path.as_deref().and_then(lang_for);
    let (sy, sx) = app.scroll;

    let end = (sy + area.height as usize).min(buf.lines.len());
    // Worst diagnostic per visible line.
    let mut marks: Vec<Option<Severity>> = vec![None; end - sy];
    for d in diags(app).iter().filter(|d| d.line >= sy && d.line < end) {
        let m = &mut marks[d.line - sy];
        *m = Some(m.map_or(d.severity, |s| s.min(d.severity)));
    }
    let mut gut = Vec::new();
    let mut rows = Vec::new();
    for y in sy..end {
        let line = &buf.lines[y];
        let current = y == buf.cur.y;
        let (mark, mark_style) = match marks[y - sy] {
            Some(sev) => (sev_mark(sev), Style::default().fg(sev_color(sev))),
            None => (" ", Style::default()),
        };
        gut.push(Line::from(vec![
            Span::styled(format!("{:>digits$}", y + 1), if current { Style::default().fg(WARN) } else { Style::default().fg(DIM) }),
            Span::styled(mark, mark_style),
            Span::raw(" "),
        ]));
        let toks = match lang {
            Some(l) => highlight_line(line, l, &mut states.get(y).copied().unwrap_or_default()),
            None => vec![Tok::Text; line.chars().count()],
        };
        // Expand tabs / control chars into cells, then crop to the view.
        let mut spans: Vec<Span> = Vec::new();
        let mut col = 0;
        for (c, t) in line.chars().zip(toks) {
            let w = char_width(c, col);
            let shown: String = match c {
                '\t' => " ".repeat(w),
                c if c.is_control() => "·".into(),
                c => c.to_string(),
            };
            if col >= sx && col + w <= sx + text.width as usize {
                spans.push(Span::styled(shown, tok_style(t)));
            }
            col += w;
        }
        let mut l = Line::from(spans);
        if current {
            l = l.style(Style::default().bg(SEL_BG));
        }
        rows.push(l);
    }
    f.render_widget(Paragraph::new(gut), Rect { width: gutter, ..area });
    f.render_widget(Paragraph::new(rows), text);

    if app.focus == Focus::Editor && app.prompt.is_none() {
        let cx = display_col(&buf.lines[buf.cur.y], buf.cur.x) - sx;
        f.set_cursor_position((text.x + cx as u16, text.y + (buf.cur.y - sy) as u16));
    }
}

fn draw_welcome(f: &mut Frame, area: Rect) {
    let logo = [
        "        _ _     ",
        "  _ __ (_) |__  ",
        " | '_ \\| | '_ \\ ",
        " | | | | | |_) |",
        " |_| |_|_|_.__/ ",
    ];
    let keys = [
        ("Ctrl+P", "all commands"),
        ("Enter", "open file / folder"),
        ("Ctrl+S", "save"),
        ("Ctrl+W", "close file"),
        ("Ctrl+Q", "quit"),
        ("Ctrl+F", "find"),
        ("Ctrl+E", "switch tree ↔ editor"),
        ("Ctrl+B", "hide / show tree"),
    ];
    let mut lines: Vec<Line> = logo.iter().map(|l| Line::from(Span::styled(*l, Style::default().fg(ACCENT)))).collect();
    lines.push(Line::from(Span::styled("a small editor for code", Style::default().fg(DIM))));
    lines.push(Line::from(""));
    for (k, d) in keys {
        lines.push(Line::from(vec![
            Span::styled(format!("{k:>8}  "), Style::default().fg(WARN)),
            Span::styled(format!("{d:<22}"), Style::default().fg(FG)),
        ]));
    }
    let h = lines.len() as u16;
    let y = area.y + area.height.saturating_sub(h) / 2;
    f.render_widget(Paragraph::new(lines).centered(), Rect { y, height: h.min(area.height), ..area });
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let bar = Style::default().bg(BAR_BG).fg(FG);
    let mode = Span::styled(" NIB ", Style::default().bg(ACCENT).fg(Color::Black).add_modifier(Modifier::BOLD));
    let mut left = vec![mode];
    let mut right = String::new();
    if let Some(b) = &app.buf {
        let name = b.path.as_ref().map_or("[no name]".into(), |p| app.rel(p));
        if icons::enabled() {
            let file = b.path.as_ref().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().into_owned());
            let (glyph, color) = icon_for(&file, false, false);
            left.push(Span::styled(format!(" {glyph}"), bar.fg(color)));
        }
        left.push(Span::styled(format!(" {name}"), bar.add_modifier(Modifier::BOLD)));
        if b.dirty {
            left.push(Span::styled(" ●", bar.fg(WARN)));
        }
        let lang = b.path.as_deref().and_then(lang_for).map_or("Text", |l| l.name);
        right = format!(
            "Ln {}, Col {}  {}  {}  {} ",
            b.cur.y + 1,
            b.cur.x + 1,
            if b.indent == "\t" { "Tabs" } else { "Spaces" },
            if b.crlf { "CRLF" } else { "LF" },
            lang
        );
    }
    left.push(Span::styled("  ^P commands", bar.fg(DIM)));
    let (errors, warnings) = diags(app).iter().fold((0, 0), |(e, w), d| match d.severity {
        Severity::Error => (e + 1, w),
        Severity::Warning => (e, w + 1),
        _ => (e, w),
    });
    let mut lsp_spans = Vec::new();
    if errors > 0 {
        lsp_spans.push(Span::styled(format!("✖ {errors}  "), bar.fg(ERR)));
    }
    if warnings > 0 {
        lsp_spans.push(Span::styled(format!("▲ {warnings}  "), bar.fg(WARN)));
    }
    if let Some((name, ready)) = app.lsp.as_ref().and_then(|l| l.label()) {
        let text = if ready { format!("{name}  ") } else { format!("{name}…  ") };
        lsp_spans.push(Span::styled(text, bar.fg(if ready { ACCENT } else { DIM })));
    }
    let used: usize = lsp_spans.iter().chain(left.iter()).map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right.chars().count());
    left.push(Span::styled(" ".repeat(pad), bar));
    left.extend(lsp_spans);
    left.push(Span::styled(right, bar.fg(DIM)));
    f.render_widget(Paragraph::new(Line::from(left)).style(bar), area);
}

fn draw_bottom(f: &mut Frame, app: &App, area: Rect) {
    let key = Style::default().fg(WARN);
    let dim = Style::default().fg(DIM);
    let line = match &app.prompt {
        Some(Prompt::Unsaved(after)) => {
            let name = app.buf.as_ref().and_then(|b| b.path.as_ref()).map_or("file".into(), |p| app.rel(p));
            let then = match after {
                After::Quit => "quitting",
                After::Close => "closing",
                After::Open(_) => "switching",
            };
            Line::from(vec![
                Span::styled(format!(" Save changes to {name} before {then}? "), Style::default().fg(WARN).add_modifier(Modifier::BOLD)),
                Span::styled("y", key), Span::styled(" save  ", dim),
                Span::styled("n", key), Span::styled(" discard  ", dim),
                Span::styled("Esc", key), Span::styled(" cancel", dim),
            ])
        }
        Some(Prompt::Find(q)) => {
            f.set_cursor_position((area.x + 7 + q.chars().count() as u16, area.y));
            Line::from(vec![
                Span::styled(" Find: ", key),
                Span::styled(q.clone(), Style::default().fg(FG)),
                Span::styled("   Enter next · Esc close", dim),
            ])
        }
        _ if !app.status.is_empty() => Line::from(Span::styled(format!(" {}", app.status), Style::default().fg(FG))),
        _ => {
            // Problem on the cursor line, if any.
            let y = app.buf.as_ref().map(|b| b.cur.y);
            match diags(app).iter().find(|d| Some(d.line) == y) {
                Some(d) => Line::from(Span::styled(
                    format!(" {} {}", severity_label(d.severity), d.message),
                    Style::default().fg(sev_color(d.severity)),
                )),
                None => Line::from(""),
            }
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/// Hover info or the completion list, next to the cursor.
fn draw_popup(f: &mut Frame, app: &App) {
    let (Some(popup), Some(b)) = (&app.popup, &app.buf) else { return };
    let text = app.text_area;
    let (sy, sx) = app.scroll;
    if b.cur.y < sy || text.width < 10 {
        return;
    }
    let screen = f.area();
    let row = text.y + (b.cur.y - sy) as u16;
    let (title, lines, anchor_x, width) = match popup {
        Popup::Hover(info) => {
            let w = 80.min(text.width as usize).max(20);
            let inner = w - 2;
            let mut lines = Vec::new();
            for l in info.lines() {
                let cs: Vec<char> = l.chars().collect();
                if cs.is_empty() {
                    lines.push(Line::from(""));
                }
                for chunk in cs.chunks(inner) {
                    lines.push(Line::from(Span::styled(chunk.iter().collect::<String>(), Style::default().fg(FG))));
                }
            }
            lines.truncate(14);
            (" Info ", lines, display_col(&b.lines[b.cur.y], b.cur.x), w)
        }
        Popup::Complete { .. } => {
            let Some((items, sel, start)) = app.completion_view() else { return };
            let label_w = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(1).min(40);
            let detail_w = items.iter().map(|i| i.detail.chars().count()).max().unwrap_or(0).min(30);
            let w = (label_w + detail_w + 5).min(text.width as usize).max(16);
            let rows = 10;
            let first = sel.saturating_sub(rows - 1);
            let lines = items
                .iter()
                .enumerate()
                .skip(first)
                .take(rows)
                .map(|(i, it)| {
                    let mut st = Style::default().fg(FG);
                    if i == sel {
                        st = st.bg(SEL_BG).fg(ACCENT).add_modifier(Modifier::BOLD);
                    }
                    let label: String = it.label.chars().take(label_w).collect();
                    let detail: String = it.detail.chars().take(w.saturating_sub(label_w + 5)).collect();
                    let pad = (w - 2).saturating_sub(label.chars().count() + detail.chars().count() + 2);
                    Line::from(vec![
                        Span::styled(format!(" {label}{}", " ".repeat(pad)), st),
                        Span::styled(format!("{detail} "), st.fg(DIM)),
                    ])
                })
                .collect();
            (" Complete ", lines, display_col(&b.lines[start.y], start.x), w)
        }
    };
    let h = lines.len() as u16 + 2;
    let x = (text.x + anchor_x.saturating_sub(sx) as u16).min(screen.width.saturating_sub(width as u16));
    let below = row + 1 + h <= screen.height.saturating_sub(2);
    let y = if below { row + 1 } else { row.saturating_sub(h) };
    let area = Rect { x, y, width: width as u16, height: h }.intersection(screen);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::default().fg(ACCENT))
                .title(Span::styled(title, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
        ),
        area,
    );
}
