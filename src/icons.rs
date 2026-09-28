//! File-type icons (Nerd Font glyphs) and their colors, as in nvim-web-devicons.
//! Set NIB_NO_ICONS=1 if the terminal font has no Nerd Font glyphs.

use ratatui_core::style::Color;

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const DEFAULT: (&str, Color) = ("\u{f15b}", rgb(0x6d8086));
const FOLDER: (&str, Color) = ("\u{f07b}", rgb(0x7aa2f7));
const FOLDER_OPEN: (&str, Color) = ("\u{f07c}", rgb(0x7aa2f7));

/// Exact file names, checked before extensions.
static NAMES: &[(&str, &str, u32)] = &[
    ("package.json", "\u{e71e}", 0xcb3837),
    ("package-lock.json", "\u{e71e}", 0x7a0d21),
    ("pnpm-lock.yaml", "\u{e71e}", 0xf9ad00),
    ("cargo.toml", "\u{e7a8}", 0xdea584),
    ("cargo.lock", "\u{e7a8}", 0xdea584),
    ("go.mod", "\u{e627}", 0x519aba),
    ("go.sum", "\u{e627}", 0x519aba),
    ("dockerfile", "\u{f308}", 0x458ee6),
    ("docker-compose.yml", "\u{f308}", 0x458ee6),
    ("docker-compose.yaml", "\u{f308}", 0x458ee6),
    ("makefile", "\u{e779}", 0x6d8086),
    (".gitignore", "\u{e702}", 0xf14c28),
    (".gitattributes", "\u{e702}", 0xf14c28),
    (".env", "\u{f462}", 0xfaf743),
    ("license", "\u{e60a}", 0xd0bf41),
    ("readme.md", "\u{f48a}", 0xdddddd),
    ("vite.config.ts", "\u{e6b3}", 0xffa800),
    ("vite.config.js", "\u{e6b3}", 0xffa800),
    ("tsconfig.json", "\u{e628}", 0x519aba),
];

static EXTS: &[(&str, &str, u32)] = &[
    ("rs", "\u{e7a8}", 0xdea584),
    ("go", "\u{e627}", 0x00add8),
    ("js", "\u{e74e}", 0xcbcb41),
    ("mjs", "\u{e74e}", 0xf1e05a),
    ("cjs", "\u{e74e}", 0xcbcb41),
    ("ts", "\u{e628}", 0x519aba),
    ("mts", "\u{e628}", 0x519aba),
    ("cts", "\u{e628}", 0x519aba),
    ("jsx", "\u{e7ba}", 0x20c2e3),
    ("tsx", "\u{e7ba}", 0x1354bf),
    ("vue", "\u{e6a0}", 0x8dc149),
    ("svelte", "\u{e697}", 0xff3e00),
    ("html", "\u{e736}", 0xe44d26),
    ("htm", "\u{e736}", 0xe44d26),
    ("css", "\u{e749}", 0x42a5f5),
    ("scss", "\u{e603}", 0xf55385),
    ("sass", "\u{e603}", 0xf55385),
    ("less", "\u{e60b}", 0x563d7c),
    ("py", "\u{e606}", 0xffbc03),
    ("pyi", "\u{e606}", 0xffbc03),
    ("sql", "\u{e706}", 0xdad8d8),
    ("db", "\u{e706}", 0xdad8d8),
    ("sqlite", "\u{e706}", 0xdad8d8),
    ("json", "\u{e60b}", 0xcbcb41),
    ("jsonc", "\u{e60b}", 0xcbcb41),
    ("md", "\u{f48a}", 0xdddddd),
    ("markdown", "\u{f48a}", 0xdddddd),
    ("yaml", "\u{e6a8}", 0x6d8086),
    ("yml", "\u{e6a8}", 0x6d8086),
    ("toml", "\u{e6b2}", 0x9c4221),
    ("ini", "\u{e615}", 0x6d8086),
    ("nib", "\u{e615}", 0x7aa2f7),
    ("conf", "\u{e615}", 0x6d8086),
    ("env", "\u{f462}", 0xfaf743),
    ("sh", "\u{e795}", 0x4d5a5e),
    ("bash", "\u{e795}", 0x89e051),
    ("zsh", "\u{e795}", 0x89e051),
    ("fish", "\u{e795}", 0x4d5a5e),
    ("lua", "\u{e620}", 0x51a0cf),
    ("c", "\u{e61e}", 0x599eff),
    ("h", "\u{f0fd}", 0xa074c4),
    ("cpp", "\u{e61d}", 0xf34b7d),
    ("cc", "\u{e61d}", 0xf34b7d),
    ("hpp", "\u{f0fd}", 0xa074c4),
    ("java", "\u{e738}", 0xcc3e44),
    ("kt", "\u{e634}", 0x7f52ff),
    ("cs", "\u{f031b}", 0x596706),
    ("dart", "\u{e798}", 0x03589c),
    ("swift", "\u{e755}", 0xe37933),
    ("zig", "\u{e6a9}", 0xf69a1b),
    ("lock", "\u{f023}", 0xbbbbbb),
    ("txt", "\u{f15c}", 0x89e051),
    ("log", "\u{f18d}", 0xdddddd),
    ("xml", "\u{f05c0}", 0xe37933),
    ("svg", "\u{f0721}", 0xffb13b),
    ("png", "\u{f1c5}", 0xa074c4),
    ("jpg", "\u{f1c5}", 0xa074c4),
    ("jpeg", "\u{f1c5}", 0xa074c4),
    ("gif", "\u{f1c5}", 0xa074c4),
    ("webp", "\u{f1c5}", 0xa074c4),
    ("ico", "\u{f1c5}", 0xcbcb41),
    ("pdf", "\u{f1c1}", 0xb30b00),
    ("zip", "\u{f410}", 0xeca517),
    ("gz", "\u{f410}", 0xeca517),
];

pub fn enabled() -> bool {
    std::env::var_os("NIB_NO_ICONS").is_none_or(|v| v.is_empty() || v == "0")
}

/// Icon glyph and color for a tree entry / file name.
pub fn icon_for(name: &str, is_dir: bool, expanded: bool) -> (&'static str, Color) {
    if is_dir {
        return if expanded { FOLDER_OPEN } else { FOLDER };
    }
    let lower = name.to_lowercase();
    if let Some((_, i, c)) = NAMES.iter().find(|(n, _, _)| *n == lower) {
        return (i, rgb(*c));
    }
    if lower.starts_with(".env") {
        return ("\u{f462}", rgb(0xfaf743));
    }
    if lower.starts_with("dockerfile") {
        return ("\u{f308}", rgb(0x458ee6));
    }
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    EXTS.iter()
        .find(|(e, _, _)| *e == ext)
        .map_or(DEFAULT, |(_, i, c)| (*i, rgb(*c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_icons_by_name_then_extension() {
        assert_eq!(icon_for("main.rs", false, false).0, "\u{e7a8}");
        assert_eq!(icon_for("App.vue", false, false), ("\u{e6a0}", rgb(0x8dc149)));
        assert_eq!(icon_for("Button.TSX", false, false).0, "\u{e7ba}", "case-insensitive");
        assert_eq!(icon_for("package.json", false, false).0, "\u{e71e}", "name beats .json");
        assert_eq!(icon_for("tsconfig.json", false, false).0, "\u{e628}");
        assert_eq!(icon_for(".env.local", false, false).0, "\u{f462}");
        assert_eq!(icon_for("Dockerfile.dev", false, false).0, "\u{f308}");
        assert_eq!(icon_for("LICENSE", false, false).0, "\u{e60a}");
        assert_eq!(icon_for("weird.xyz", false, false), DEFAULT);
        assert_eq!(icon_for("noext", false, false), DEFAULT);
        assert_eq!(icon_for("src", true, false), FOLDER);
        assert_eq!(icon_for("src", true, true), FOLDER_OPEN);
    }

    #[test]
    fn every_icon_is_one_cell_wide() {
        use unicode_width::UnicodeWidthStr;
        for (_, i, _) in NAMES.iter().chain(EXTS) {
            assert_eq!(i.width(), 1, "{i:?}");
        }
    }
}
