<div align="center">

```
 ███╗   ██╗██╗██████╗
 ████╗  ██║██║██╔══██╗
 ██╔██╗ ██║██║██████╔╝
 ██║╚██╗██║██║██╔══██╗
 ██║ ╚████║██║██████╔╝
 ╚═╝  ╚═══╝╚═╝╚═════╝
```

**A lightweight terminal code editor with a file tree.**
Written in Rust. One small binary, and it's careful with your files.

</div>

---

## Install

```bash
make install        # cargo install --path . --locked  →  ~/.cargo/bin/nib
```

## Usage

```bash
nib                 # open the current folder
nib <folder>        # open a folder in the file tree
nib <file>          # open a file (a new name creates it on first save)
```

Press **`Ctrl+P`** anywhere to open the command palette: type to filter
(e.g. `sav`), pick with `↑` `↓`, run with `Enter`, close with `Esc`. Every
command also has a direct shortcut, listed below and next to each command in
the palette.

## Keys

| Where  | Key | Action |
|--------|-----|--------|
| Global | `Ctrl+P` | Command palette |
|        | `Ctrl+S` | Save now (auto-save usually does it for you) |
|        | `Ctrl+W` | Close file |
|        | `Ctrl+Q` | Quit |
|        | `Ctrl+F` | Find (`Enter` = next, `Esc` = close) |
|        | `Ctrl+E` | Switch between tree and editor |
|        | `Ctrl+B` | Show/hide sidebar |
|        | `Ctrl+R` | Refresh tree |
| Editor | `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
|        | `Ctrl+K` / `Ctrl+U` | Cut line / paste line |
|        | `Ctrl+←` / `Ctrl+→` | Jump by word |
|        | `Home` | Toggle first non-blank / column 0 |
|        | `Ctrl+Home` / `Ctrl+End` | Start / end of file |
|        | `PgUp` / `PgDn` | Page up / down |
|        | `Tab` | Indent to the next tab stop |
|        | `Esc` | Go to the tree |
| Tree   | `↑` `↓` / `j` `k` | Move |
|        | `Enter` / `l` / `→` | Open file or expand folder |
|        | `←` / `h` | Collapse folder / go to parent |
| Mouse  | click, scroll | Select, open, place cursor, scroll |

## Safety

- **Atomic saves:** nib writes a temp file and renames it over the original, keeping file permissions.
- **Line endings kept:** CRLF line endings and the trailing newline (or its absence) are preserved.
- **Won't open** binary, non-UTF-8, or larger-than-50 MB files, so it never corrupts them.
- **Auto-save:** saves 1 s after you stop typing, and before switching files, closing,
  quitting or when the terminal loses focus. Undo still works after a save. Turn it off
  with the `Toggle auto-save` command (`Ctrl+P`); then unsaved changes prompt
  `y` save / `n` discard / `Esc` cancel. If a save fails (e.g. read-only file) nib says
  so once and asks before discarding anything.

## File icons

The file tree and status bar show a colored icon per language (Rust, Go, JS, TS, React,
Vue, Svelte, HTML, CSS/SCSS, Python, SQL, JSON, YAML, TOML, Markdown, Docker, git, …),
the same glyphs as nvim-web-devicons. They need a [Nerd Font](https://www.nerdfonts.com/)
in your terminal; if you see boxes instead, run with `NIB_NO_ICONS=1`.

## Syntax highlighting

Hand-written scanners — no grammar files or regexes — so highlighting costs almost
nothing. After an edit only the changed lines are re-scanned.

| Language | Highlights |
|---|---|
| JavaScript, TypeScript, React (JSX/TSX) | keywords, template strings (multi-line), decorators, JSX tags/props |
| HTML | tags, attributes, entities, comments, inline `<script>` and `<style>` |
| Vue, Svelte | template tags, `v-`/`@`/`:` and `on:` directives, `{{ }}` / `{#if}` blocks, `<script>` as TS, `<style>` as SCSS |
| CSS, SCSS, Less | selectors, properties, colors, units, `@rules`, `!important` |
| Python | keywords, decorators, `"""` docstrings, f-strings |
| SQL | keywords and types (any case), strings, comments |
| JSON, YAML, TOML, INI, .env, Dockerfile, Makefile | keys vs values, sections, instructions |
| Markdown | headings, lists, code spans and fences, bold, links |
| Rust, Go, C/C++, Java/Kotlin/C#/Dart/Swift, Zig, Shell, Lua | keywords, strings, comments, numbers, calls, types |

## Limitations

- One file open at a time.
- No mouse text selection. Use your terminal's own selection to copy; pasting works through bracketed paste.
- Cut and paste works on whole lines only.

## Development

```bash
cargo test
```

## License

[MIT](LICENSE)
