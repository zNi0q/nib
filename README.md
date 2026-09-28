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

Linux and macOS (Windows: use WSL). Needs Rust ([rustup.rs](https://rustup.rs)).

```bash
git clone <this repo> && cd nib
./install.sh                          # builds nib, creates the config, asks which language servers to add
./install.sh --lsp typescript,python  # or pick them up front
./install.sh --lsp all                # every language server preset
./install.sh --no-lsp                 # just the editor
```

Language servers install with the tool they need (npm, go or rustup). npm
installs go to `~/.local` when the global npm folder needs root, so no `sudo`.
`make install` builds nib alone.

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

## Language servers (LSP)

Errors as you type, go to definition, hover info and autocomplete come from
language servers, added as **plugins**: small TOML files in
`~/.config/nib/plugins/`. Plugins are plain data (no scripting runtime), so an
installed plugin costs nothing until you open a matching file.

```bash
nib plugin install typescript vue python   # install the servers and their plugins
nib plugin install all                     # every preset
nib plugin add typescript                  # only the plugin file (server installed yourself)
nib plugin list                        # installed plugins, and whether each server is found
nib plugin new mylang                  # commented template for any other server
nib plugin remove vue
```

Presets: `typescript` (TypeScript 7's native server — `npm i -g typescript@latest`),
`vue`, `svelte`, `html`, `css`, `json`, `python` (pyright), `sql` (sqls), `rust`, `go`.
Servers are looked up on `PATH` and in Mason, `~/go/bin`, `~/.cargo/bin`, `~/.bun/bin`,
`~/.npm-global/bin` and `~/.local/bin`.

| Key | Action |
|---|---|
| `F12` / `Alt+←` | Go to definition / go back |
| `F1` | Hover info (types, docs) |
| `Ctrl+Space` | Autocomplete (also opens after `.`); `↑` `↓`, `Enter`/`Tab`, `Esc` |
| `F8` | Next problem |
| `Ctrl+P` → `LSP: status` | Running servers and the RAM each one uses |

Problems show as `●` (error) / `▲` (warning) next to the line number, with counts in the
status bar and the message at the bottom when the cursor is on that line.

**How it stays light:**

- a server starts only when you open a file it handles, and is shared per project;
- it is **stopped automatically** `idle_timeout` seconds (default 120) after its last
  file is closed — that's where most RAM goes (Node-based servers use hundreds of MB);
- Node-based presets cap their heap with `NODE_OPTIONS=--max-old-space-size=1024`;
- edits are sent once, 300 ms after you stop typing, not per keystroke;
- server messages and key presses share one queue, so nib sleeps with zero CPU when idle;
- stored diagnostics and completion lists are capped; server logs are discarded.

nib itself stays around 4 MB of RAM.

A plugin file:

```toml
name = "python"
command = "pyright-langserver"
args = ["--stdio"]
extensions = ["py", "pyi"]
root_markers = ["pyproject.toml", ".git"]
idle_timeout = 120
env = { NODE_OPTIONS = "--max-old-space-size=1024" }
# language_ids = { ext = "id" }, init_options = { ... }, enabled = true, install = "..."
```

## Configuration

Everything is set in one file, `~/.config/nib/config.nib` (TOML syntax). Create a
fully commented one with `nib config`, check it with `nib config check`. Save it
inside nib and it applies instantly; a mistake shows a message with the line and
falls back to the default, so nib always starts.

```toml
[editor]
tab_width = 4
autosave = true
autosave_delay = 1000     # ms
sidebar_width = 32
icons = true
line_numbers = true

[keys]                    # command = "key" or ["key1", "key2"]; "none" removes it
save = "ctrl+s"
definition = ["f12", "ctrl+g"]
hover = "none"

[theme]
name = "tokyonight"       # tokyonight | catppuccin | gruvbox
keyword = "#bb9af7"       # override any color: text dim accent bar selection warning
                          # error keyword string comment number function type tag attribute
[lsp]
enabled = true
idle_timeout = 120        # seconds before an unused language server is stopped
```

Commands for `[keys]`: `palette save find definition back hover complete next_problem
undo redo cut_line paste_line switch_focus toggle_sidebar refresh toggle_autosave
lsp_status lsp_restart close quit`. Keys: `ctrl`/`alt`/`shift` + a letter, digit,
punctuation, `f1`–`f12`, `space`, `tab`, `enter`, `esc`, arrows, `home`, `end`,
`pageup`, `pagedown`, `delete`, `backspace`. Keys without `ctrl`/`alt` are only allowed
for `f1`–`f12` (the rest are for typing). The palette always shows your current keys.

## File icons

The file tree and status bar show a colored icon per language (Rust, Go, JS, TS, React,
Vue, Svelte, HTML, CSS/SCSS, Python, SQL, JSON, YAML, TOML, Markdown, Docker, git, …),
the same glyphs as nvim-web-devicons. They need a [Nerd Font](https://www.nerdfonts.com/)
in your terminal; if you see boxes instead, set `icons = false` in the config.

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
