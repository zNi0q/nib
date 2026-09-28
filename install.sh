#!/bin/sh
# nib installer — builds nib, creates its config and (optionally) installs
# language servers. Works on Linux and macOS (and WSL).
#
#   ./install.sh                       # asks which language servers to install
#   ./install.sh --lsp typescript,python
#   ./install.sh --lsp all             # every preset
#   ./install.sh --no-lsp              # just nib
#
# Run it from a clone of the repo, or anywhere with NIB_REPO=<git url>.
set -eu

LSP=""
ASK=1
for arg in "$@"; do
    case "$arg" in
        --lsp=*) LSP="${arg#--lsp=}"; ASK=0 ;;
        --lsp) ASK=0; LSP="__next__" ;;
        --no-lsp) LSP=""; ASK=0 ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *)
            if [ "$LSP" = "__next__" ]; then LSP="$arg"; else echo "unknown option: $arg" >&2; exit 2; fi ;;
    esac
done
[ "$LSP" = "__next__" ] && { echo "--lsp needs a value, e.g. --lsp typescript,python" >&2; exit 2; }

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
    Linux|Darwin) ;;
    *) die "nib supports Linux and macOS (on Windows, use WSL)" ;;
esac

# Rust toolchain.
if ! command -v cargo >/dev/null 2>&1; then
    [ -x "$HOME/.cargo/bin/cargo" ] && PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
    die "Rust is needed to build nib. Install it from https://rustup.rs, then run this again."
fi

# Build and install nib.
here="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$here/Cargo.toml" ] && grep -q '^name = "nib"' "$here/Cargo.toml"; then
    say "Building nib from $here"
    cargo install --quiet --locked --path "$here"
elif [ -n "${NIB_REPO:-}" ]; then
    say "Building nib from $NIB_REPO"
    cargo install --quiet --locked --git "$NIB_REPO" nib
else
    die "run this script from the nib repo, or set NIB_REPO to its git URL"
fi

bin_dir="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
NIB="$bin_dir/nib"
[ -x "$NIB" ] || die "nib was not installed to $bin_dir"
say "Installed $("$NIB" --version) to $NIB"

# Config file (kept if it already exists).
"$NIB" config >/dev/null
say "Config: $("$NIB" config path)"

# Language servers.
if [ "$ASK" = 1 ]; then
    presets="$("$NIB" plugin presets)"
    if [ -t 0 ]; then
        echo
        echo "Language servers add errors, go-to-definition and autocomplete."
        echo "Available: $presets"
        printf "Install which? (comma-separated, 'all', or Enter for none): "
        read -r LSP || LSP=""
    fi
fi
if [ -n "$LSP" ]; then
    say "Installing language servers: $LSP"
    # shellcheck disable=SC2046 — word splitting of the list is intended
    "$NIB" plugin install $(echo "$LSP" | tr ',' ' ') || echo "Some language servers were not installed (see above)."
fi

case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo; echo "Add nib to your PATH:  export PATH=\"$bin_dir:\$PATH\"" ;;
esac
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) [ -n "$LSP" ] && echo "Language servers installed with npm may be in ~/.local/bin (nib finds them there)." ;;
esac
echo
say "Done. Open a folder with:  nib <folder>"
