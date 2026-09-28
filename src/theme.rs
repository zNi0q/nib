//! Color themes. Backgrounds stay the terminal's own except for bars and the
//! selection, so nib blends into the terminal theme.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    pub bar: Color,
    pub selection: Color,
    pub warning: Color,
    pub error: Color,
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub function: Color,
    pub ty: Color,
    pub tag: Color,
    pub attribute: Color,
}

const fn hex(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const TOKYONIGHT: Theme = Theme {
    text: hex(0xc0caf5),
    dim: hex(0x565f89),
    accent: hex(0x7aa2f7),
    bar: hex(0x24283b),
    selection: hex(0x292e42),
    warning: hex(0xe0af68),
    error: hex(0xf7768e),
    keyword: hex(0xbb9af7),
    string: hex(0x9ece6a),
    comment: hex(0x565f89),
    number: hex(0xff9e64),
    function: hex(0x7aa2f7),
    ty: hex(0x2ac3de),
    tag: hex(0xf7768e),
    attribute: hex(0xe0af68),
};

pub const CATPPUCCIN: Theme = Theme {
    text: hex(0xcdd6f4),
    dim: hex(0x6c7086),
    accent: hex(0x89b4fa),
    bar: hex(0x181825),
    selection: hex(0x313244),
    warning: hex(0xf9e2af),
    error: hex(0xf38ba8),
    keyword: hex(0xcba6f7),
    string: hex(0xa6e3a1),
    comment: hex(0x6c7086),
    number: hex(0xfab387),
    function: hex(0x89b4fa),
    ty: hex(0xf9e2af),
    tag: hex(0x89dceb),
    attribute: hex(0xfab387),
};

pub const GRUVBOX: Theme = Theme {
    text: hex(0xebdbb2),
    dim: hex(0x928374),
    accent: hex(0x83a598),
    bar: hex(0x3c3836),
    selection: hex(0x504945),
    warning: hex(0xfabd2f),
    error: hex(0xfb4934),
    keyword: hex(0xfb4934),
    string: hex(0xb8bb26),
    comment: hex(0x928374),
    number: hex(0xd3869b),
    function: hex(0x8ec07c),
    ty: hex(0xfabd2f),
    tag: hex(0x83a598),
    attribute: hex(0xfe8019),
};

pub const NAMES: &[&str] = &["tokyonight", "catppuccin", "gruvbox"];

pub fn builtin(name: &str) -> Option<Theme> {
    match name.to_lowercase().as_str() {
        "tokyonight" | "tokyo-night" => Some(TOKYONIGHT),
        "catppuccin" | "catppuccin-mocha" => Some(CATPPUCCIN),
        "gruvbox" | "gruvbox-dark" => Some(GRUVBOX),
        _ => None,
    }
}

/// "#rrggbb" (or "rrggbb") to a color.
pub fn parse_color(s: &str) -> Result<Color, String> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return Err(format!("{s:?} is not a #rrggbb color"));
    }
    u32::from_str_radix(h, 16).map(hex).map_err(|_| format!("{s:?} is not a #rrggbb color"))
}

impl Theme {
    /// Set one color by its config name.
    pub fn set(&mut self, name: &str, c: Color) -> Result<(), String> {
        let slot = match name {
            "text" => &mut self.text,
            "dim" => &mut self.dim,
            "accent" => &mut self.accent,
            "bar" => &mut self.bar,
            "selection" => &mut self.selection,
            "warning" => &mut self.warning,
            "error" => &mut self.error,
            "keyword" => &mut self.keyword,
            "string" => &mut self.string,
            "comment" => &mut self.comment,
            "number" => &mut self.number,
            "function" => &mut self.function,
            "type" => &mut self.ty,
            "tag" => &mut self.tag,
            "attribute" => &mut self.attribute,
            _ => return Err(format!("unknown color {name:?}")),
        };
        *slot = c;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_themes() {
        assert_eq!(parse_color("#fb4934"), Ok(Color::Rgb(251, 73, 52)));
        assert_eq!(parse_color("010203"), Ok(Color::Rgb(1, 2, 3)));
        assert!(parse_color("#12345").is_err() && parse_color("#gggggg").is_err());
        for n in NAMES {
            assert!(builtin(n).is_some(), "{n}");
        }
        let mut t = GRUVBOX;
        t.set("keyword", Color::Rgb(1, 2, 3)).unwrap();
        assert_eq!(t.keyword, Color::Rgb(1, 2, 3));
        assert!(t.set("nope", Color::Reset).is_err());
    }
}
