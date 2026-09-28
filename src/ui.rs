use crate::app::{severity_label, After, App, Focus, Popup, Prompt};
use crate::buffer::{char_width, display_col};
use crate::highlight::{highlight_line, lang_for, Tok};
use crate::icons::{self, icon_for};
use crate::lsp::{Diag, Severity};
use crate::screen::{fg, Color, Rect, Screen, Style};
use crate::theme::Theme;

fn sev_color(th: &Theme, s: Severity) -> Color {
    match s {
        Severity::Error => th.error,
        Severity::Warning => th.warning,
        _ => th.accent,
    }
}

fn sev_mark(s: Severity) -> &'static str {
    match s {
        Severity::Error => "●",
        Severity::Warning => "▲",
        _ => "·",
    }
}

fn diags(app: &App) -> &[Diag] {
    match (&app.lsp, app.buf.as_ref().and_then(|b| b.path.as_ref())) {
        (Some(l), Some(p)) => l.diags_for(p),
        _ => &[],
    }
}

fn tok_style(th: &Theme, t: Tok) -> Style {
    match t {
        Tok::Text => fg(th.text),
        Tok::Keyword => fg(th.keyword),
        Tok::Str => fg(th.string),
        Tok::Comment => fg(th.comment).italic(),
        Tok::Number => fg(th.number),
        Tok::Func => fg(th.function),
        Tok::Type => fg(th.ty),
        Tok::Tag => fg(th.tag),
        Tok::Attr => fg(th.attribute),
    }
}

fn width_of(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

pub fn draw(scr: &mut Screen, app: &mut App) {
    let full = scr.area();
    let main = Rect { height: full.height.saturating_sub(2), ..full };
    let status = Rect { y: full.height.saturating_sub(2), height: 1.min(full.height), ..full };
    let bottom = Rect { y: full.height.saturating_sub(1), height: 1.min(full.height), ..full };
    let editor = if app.show_tree {
        let tree_w = app.settings.sidebar_width.min(main.width.saturating_sub(10));
        draw_tree(scr, app, Rect { width: tree_w, ..main });
        Rect { x: main.x + tree_w, width: main.width - tree_w, ..main }
    } else {
        app.tree_area = Rect::default();
        main
    };
    draw_editor(scr, app, editor);
    draw_status(scr, app, status);
    draw_bottom(scr, app, bottom);
    if let Some(Prompt::Palette { query, sel }) = &app.prompt {
        draw_palette(scr, app, query, *sel);
    } else {
        draw_popup(scr, app);
    }
}

fn draw_palette(scr: &mut Screen, app: &App, query: &str, sel: usize) {
    let th = &app.theme;
    let matches = App::palette_matches(query);
    let screen = scr.area();
    let w = 56.min(screen.width.saturating_sub(4));
    let h = (matches.len().max(1) as u16 + 4).min(screen.height.saturating_sub(2));
    let area = Rect { x: (screen.width - w) / 2, y: screen.height / 6, width: w, height: h };
    scr.boxed(area, fg(th.accent), " Commands ", fg(th.accent).bold());
    let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2) };
    let right = inner.right();
    scr.put_spans(inner.x, inner.y, &[(" > ", fg(th.warning)), (query, fg(th.text).bold())], right);
    scr.put(inner.x, inner.y + 1, &"─".repeat(inner.width as usize), fg(th.dim), right);
    if matches.is_empty() {
        scr.put(inner.x, inner.y + 2, " no matching command", fg(th.dim), right);
    }
    let rows = inner.height.saturating_sub(2) as usize;
    let first = sel.saturating_sub(rows.saturating_sub(1));
    for (row, (i, c)) in matches.iter().enumerate().skip(first).take(rows).enumerate() {
        let y = inner.y + 2 + row as u16;
        let keys = app.keymap.label(c.cmd);
        let mut style = fg(th.text);
        if i == sel {
            style = style.bg(th.selection).fg(th.accent).bold();
            scr.fill(Rect { y, height: 1, ..inner }, style);
        }
        scr.put(inner.x, y, &format!(" {}", c.name), style, right);
        let kx = right.saturating_sub(width_of(&keys) as u16 + 2);
        scr.put(kx, y, &keys, style.fg(th.dim), right);
    }
    scr.set_cursor(inner.x + 3 + query.chars().count() as u16, inner.y);
}

fn draw_tree(scr: &mut Screen, app: &mut App, area: Rect) {
    let th = app.theme;
    if area.width < 2 || area.height == 0 {
        return;
    }
    let focused = app.focus == Focus::Tree;
    let root = app.tree.root.file_name().map_or("/".into(), |n| n.to_string_lossy().into_owned());
    let border_x = area.right() - 1;
    for y in area.y..area.bottom() {
        scr.put(border_x, y, "│", fg(if focused { th.accent } else { th.dim }), area.right());
    }
    scr.put(area.x, area.y, &format!(" {root} "), fg(th.accent).bold(), border_x);
    let list = Rect { x: area.x, y: area.y + 1, width: area.width - 1, height: area.height.saturating_sub(1) };
    app.tree_area = list;
    app.follow_cursor();

    let open = app.buf.as_ref().and_then(|b| b.path.clone());
    let show_icons = app.settings.icons && icons::enabled();
    let rows = app.tree.items.iter().enumerate().skip(app.tree.scroll).take(list.height as usize);
    for (row, (i, e)) in rows.enumerate() {
        let y = list.y + row as u16;
        let (chevron, name, mut style) = if e.is_dir {
            (if e.expanded { "▾ " } else { "▸ " }, format!("{}/", e.name), fg(th.accent))
        } else {
            let s = if open.as_ref() == Some(&e.path) { fg(th.warning) } else { fg(th.text) };
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
            style = style.bg(th.selection).bold();
            icon_style = icon_style.bg(th.selection);
            if focused {
                style = style.reversed();
                icon_style = icon_style.reversed();
            }
            scr.fill(Rect { y, height: 1, ..list }, style);
        }
        let lead = format!(" {}{chevron}", "  ".repeat(e.depth));
        scr.put_spans(list.x, y, &[(&lead, style), (&icon, icon_style), (&name, style)], list.right());
    }
}

fn draw_editor(scr: &mut Screen, app: &mut App, area: Rect) {
    let th = app.theme;
    if app.buf.is_none() {
        app.text_area = area;
        return draw_welcome(scr, &th, area);
    }
    let numbers = app.settings.line_numbers;
    let digits = if numbers { app.buf.as_ref().unwrap().lines.len().to_string().len().max(3) } else { 0 };
    let gutter = digits as u16 + 2;
    let text = Rect { x: area.x + gutter, width: area.width.saturating_sub(gutter), ..area };
    app.text_area = text;
    app.follow_cursor();
    let states = app.line_states().to_vec();
    let buf = app.buf.as_ref().unwrap();
    let lang = buf.path.as_deref().and_then(lang_for);
    let (sy, sx) = app.scroll;

    let end = (sy + area.height as usize).min(buf.lines.len());
    let mut marks: Vec<Option<Severity>> = vec![None; end - sy];
    for d in diags(app).iter().filter(|d| d.line >= sy && d.line < end) {
        let m = &mut marks[d.line - sy];
        *m = Some(m.map_or(d.severity, |s| s.min(d.severity)));
    }
    for y in sy..end {
        let row = area.y + (y - sy) as u16;
        let line = &buf.lines[y];
        let current = y == buf.cur.y;
        let (mark, mark_style) = match marks[y - sy] {
            Some(sev) => (sev_mark(sev), fg(sev_color(&th, sev))),
            None => (" ", Style::default()),
        };
        let num = if numbers { format!("{:>digits$}", y + 1) } else { String::new() };
        let num_style = fg(if current { th.warning } else { th.dim });
        scr.put_spans(area.x, row, &[(&num, num_style), (mark, mark_style)], text.x);

        let base = if current { Style::default().bg(th.selection) } else { Style::default() };
        if current {
            scr.fill(Rect { y: row, height: 1, ..text }, base);
        }
        let toks = match lang {
            Some(l) => highlight_line(line, l, &mut states.get(y).copied().unwrap_or_default()),
            None => vec![Tok::Text; line.chars().count()],
        };
        let mut col = 0;
        let mut tmp = [0u8; 4];
        for (c, t) in line.chars().zip(toks) {
            let w = char_width(c, col);
            if col >= sx && col + w <= sx + text.width as usize {
                let x = text.x + (col - sx) as u16;
                let style = tok_style(&th, t).bg(base.bg);
                match c {
                    '\t' => {
                        scr.put(x, row, &" ".repeat(w), style, text.right());
                    }
                    c if c.is_control() => {
                        scr.put(x, row, "·", style, text.right());
                    }
                    c => {
                        scr.put(x, row, c.encode_utf8(&mut tmp), style, text.right());
                    }
                }
            }
            col += w;
        }
    }

    if app.focus == Focus::Editor && app.prompt.is_none() {
        let cx = display_col(&buf.lines[buf.cur.y], buf.cur.x) - sx;
        scr.set_cursor(text.x + cx as u16, text.y + (buf.cur.y - sy) as u16);
    }
}

fn draw_welcome(scr: &mut Screen, th: &Theme, area: Rect) {
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
    let mut lines: Vec<Vec<(String, Style)>> = logo.iter().map(|l| vec![(l.to_string(), fg(th.accent))]).collect();
    lines.push(vec![("a small editor for code".into(), fg(th.dim))]);
    lines.push(Vec::new());
    for (k, d) in keys {
        lines.push(vec![(format!("{k:>8}  "), fg(th.warning)), (format!("{d:<22}"), fg(th.text))]);
    }
    let top = area.y + area.height.saturating_sub(lines.len() as u16) / 2;
    for (i, spans) in lines.iter().enumerate() {
        let y = top + i as u16;
        if y >= area.bottom() {
            break;
        }
        let w: usize = spans.iter().map(|(t, _)| width_of(t)).sum();
        let x = area.x + area.width.saturating_sub(w as u16) / 2;
        let spans: Vec<(&str, Style)> = spans.iter().map(|(t, s)| (t.as_str(), *s)).collect();
        scr.put_spans(x, y, &spans, area.right());
    }
}

fn draw_status(scr: &mut Screen, app: &App, area: Rect) {
    let th = app.theme;
    let bar = Style::default().bg(th.bar).fg(th.text);
    scr.fill(area, bar);
    let mut left: Vec<(String, Style)> = vec![(" NIB ".into(), Style::default().bg(th.accent).fg(Color::Black).bold())];
    let mut right = String::new();
    if let Some(b) = &app.buf {
        let name = b.path.as_ref().map_or("[no name]".into(), |p| app.rel(p));
        if app.settings.icons && icons::enabled() {
            let file = b.path.as_ref().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().into_owned());
            let (glyph, color) = icon_for(&file, false, false);
            left.push((format!(" {glyph}"), bar.fg(color)));
        }
        left.push((format!(" {name}"), bar.bold()));
        if b.dirty {
            left.push((" ●".into(), bar.fg(th.warning)));
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
    left.push(("  ^P commands".into(), bar.fg(th.dim)));
    let (errors, warnings) = diags(app).iter().fold((0, 0), |(e, w), d| match d.severity {
        Severity::Error => (e + 1, w),
        Severity::Warning => (e, w + 1),
        _ => (e, w),
    });
    let mut lsp: Vec<(String, Style)> = Vec::new();
    if errors > 0 {
        lsp.push((format!("✖ {errors}  "), bar.fg(th.error)));
    }
    if warnings > 0 {
        lsp.push((format!("▲ {warnings}  "), bar.fg(th.warning)));
    }
    if let Some((name, ready)) = app.lsp.as_ref().and_then(|l| l.label()) {
        let text = if ready { format!("{name}  ") } else { format!("{name}…  ") };
        lsp.push((text, bar.fg(if ready { th.accent } else { th.dim })));
    }
    lsp.push((right, bar.fg(th.dim)));
    let spans: Vec<(&str, Style)> = left.iter().map(|(t, s)| (t.as_str(), *s)).collect();
    scr.put_spans(area.x, area.y, &spans, area.right());
    let right_w: usize = lsp.iter().map(|(t, _)| width_of(t)).sum();
    let rx = area.right().saturating_sub(right_w as u16);
    let spans: Vec<(&str, Style)> = lsp.iter().map(|(t, s)| (t.as_str(), *s)).collect();
    scr.put_spans(rx, area.y, &spans, area.right());
}

fn draw_bottom(scr: &mut Screen, app: &App, area: Rect) {
    let th = app.theme;
    let key = fg(th.warning);
    let dim = fg(th.dim);
    let right = area.right();
    match &app.prompt {
        Some(Prompt::Unsaved(after)) => {
            let name = app.buf.as_ref().and_then(|b| b.path.as_ref()).map_or("file".into(), |p| app.rel(p));
            let then = match after {
                After::Quit => "quitting",
                After::Close => "closing",
                After::Open(_) => "switching",
            };
            let question = format!(" Save changes to {name} before {then}? ");
            let spans = [
                (question.as_str(), fg(th.warning).bold()),
                ("y", key),
                (" save  ", dim),
                ("n", key),
                (" discard  ", dim),
                ("Esc", key),
                (" cancel", dim),
            ];
            scr.put_spans(area.x, area.y, &spans, right);
        }
        Some(Prompt::Find(q)) => {
            scr.set_cursor(area.x + 7 + q.chars().count() as u16, area.y);
            scr.put_spans(area.x, area.y, &[(" Find: ", key), (q, fg(th.text)), ("   Enter next · Esc close", dim)], right);
        }
        _ if !app.status.is_empty() => {
            scr.put(area.x, area.y, &format!(" {}", app.status), fg(th.text), right);
        }
        _ => {
            let y = app.buf.as_ref().map(|b| b.cur.y);
            if let Some(d) = diags(app).iter().find(|d| Some(d.line) == y) {
                let text = format!(" {} {}", severity_label(d.severity), d.message);
                scr.put(area.x, area.y, &text, fg(sev_color(&th, d.severity)), right);
            }
        }
    }
}

fn draw_popup(scr: &mut Screen, app: &App) {
    let th = app.theme;
    let (Some(popup), Some(b)) = (&app.popup, &app.buf) else { return };
    let text = app.text_area;
    let (sy, sx) = app.scroll;
    if b.cur.y < sy || text.width < 10 {
        return;
    }
    let screen = scr.area();
    let row = text.y + (b.cur.y - sy) as u16;
    let (title, lines, anchor_x, width) = match popup {
        Popup::Hover(info) => {
            let w = 80.min(text.width as usize).max(20);
            let inner = w - 2;
            let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
            for l in info.lines() {
                let cs: Vec<char> = l.chars().collect();
                if cs.is_empty() {
                    lines.push(Vec::new());
                }
                for chunk in cs.chunks(inner) {
                    lines.push(vec![(chunk.iter().collect(), fg(th.text))]);
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
                    let mut st = fg(th.text);
                    if i == sel {
                        st = st.bg(th.selection).fg(th.accent).bold();
                    }
                    let label: String = it.label.chars().take(label_w).collect();
                    let detail: String = it.detail.chars().take(w.saturating_sub(label_w + 5)).collect();
                    let pad = (w - 2).saturating_sub(label.chars().count() + detail.chars().count() + 2);
                    vec![(format!(" {label}{}", " ".repeat(pad)), st), (format!("{detail} "), st.fg(th.dim))]
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
    scr.boxed(area, fg(th.accent), title, fg(th.accent).bold());
    for (i, spans) in lines.iter().enumerate() {
        let y = area.y + 1 + i as u16;
        if y + 1 >= area.bottom() {
            break;
        }
        let spans: Vec<(&str, Style)> = spans.iter().map(|(t, s)| (t.as_str(), *s)).collect();
        scr.put_spans(area.x + 1, y, &spans, area.right().saturating_sub(1));
    }
}
