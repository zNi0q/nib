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
|        | `Ctrl+S` | Save |
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
- **Asks before losing work:** unsaved changes prompt `y` save / `n` discard / `Esc` cancel.

## Syntax highlighting

Rust, Go, C/C++, JS/TS/Vue/Svelte, Java/Kotlin/C#/Dart/Swift, Python, Zig, Shell, Lua,
SQL, CSS, HTML/XML, JSON, TOML/YAML/INI/Makefile/Dockerfile/.env.

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
