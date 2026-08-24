#!/bin/sh
# gvsn (Go Version Manager) -- Linux and macOS uninstaller
#
# Removes every trace of gvsn from the system:
#   - The data directory (~/.gvsn by default): all installed Go versions,
#     the version/current pointers, the lock file, and the tmp/ staging area.
#   - The gvsn binary itself.
#   - Every gvsn-managed block from your shell profiles (bash, zsh, fish) and
#     login profiles (~/.profile, ~/.zprofile), wherever they are found -
#     not just the currently detected shell.
#   - Leftover temp files from an install/upgrade that was interrupted
#     mid-way (these are not cleaned up automatically if the process is
#     killed or crashes).
#
# Usage (one-liner):
#   curl -fsSL https://raw.githubusercontent.com/jhonsferg/gvsn/main/install/uninstall.sh | sh
#
# Pass flags through when piping by using `sh -s --`:
#   curl -fsSL .../uninstall.sh | sh -s -- --force
#   curl -fsSL .../uninstall.sh | sh -s -- --dry-run
#
# Or, since a piped script can't prompt interactively without a real
# terminal, set these before piping instead:
#   GVSN_UNINSTALL_FORCE=1 curl -fsSL .../uninstall.sh | sh
#   GVSN_UNINSTALL_DRY_RUN=1 curl -fsSL .../uninstall.sh | sh
#
# Customise the locations to clean (only needed if you used these at
# install time):
#   GVSN_DIR=$HOME/.gvsn GVSN_INSTALL_DIR=$HOME/.local/bin sh uninstall.sh

set -eu

GVSN_DIR_PATH="${GVSN_DIR:-$HOME/.gvsn}"
INSTALL_DIR="${GVSN_INSTALL_DIR:-$HOME/.local/bin}"

FORCE=0
[ "${GVSN_UNINSTALL_FORCE:-}" = "1" ] && FORCE=1
DRY_RUN=0
[ "${GVSN_UNINSTALL_DRY_RUN:-}" = "1" ] && DRY_RUN=1
for arg in "$@"; do
    case "$arg" in
        --force | -f) FORCE=1 ;;
        --dry-run | -n) DRY_RUN=1 ;;
    esac
done

# -- Terminal helpers ----------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_CYAN='\033[0;36m'
    C_GREEN='\033[0;32m'
    C_YELLOW='\033[1;33m'
    C_RED='\033[0;31m'
    C_BOLD='\033[1m'
    C_RESET='\033[0m'
else
    C_CYAN='' C_GREEN='' C_YELLOW='' C_RED='' C_BOLD='' C_RESET=''
fi

step() { printf "  ${C_CYAN}->${C_RESET} %s\n" "$1"; }
ok()   { printf "  ${C_GREEN}v ${C_RESET} %s\n" "$1"; }
die()  { printf "  ${C_RED}x ${C_RESET} %s\n" "$1" >&2; exit 1; }

printf "\n  ${C_BOLD}${C_RED}gvsn${C_RESET}${C_BOLD} -- uninstaller${C_RESET}\n\n"

# -- Strip gvsn-managed blocks from a profile file -------------------------------
#
# Mirrors the block model used by gvsn's own profile.rs: a gvsn block starts at
# one of the recognised marker lines and ends at the next blank line (or EOF).
# Only gvsn-managed content is touched; everything else in the file - and the
# file itself if gvsn never touched it - is left exactly as it was.
#
# Returns success (0) if the file was changed, failure (1) otherwise.
strip_gvsn_blocks() {
    file="$1"
    [ -f "$file" ] || return 1
    tmp="$(mktemp)"
    awk '
        /^# gvsn init$/ || /^# gvsn wrapper$/ || /^# gvsn path$/ || /^# gvsn: binary location$/ {
            in_block = 1
            next
        }
        in_block && NF == 0 { in_block = 0; next }
        in_block { next }
        { print }
    ' "$file" > "$tmp"
    if cmp -s "$file" "$tmp"; then
        rm -f "$tmp"
        return 1
    fi
    mv "$tmp" "$file"
    return 0
}

# -- Locate the binary -----------------------------------------------------
GVSN_BIN="$(command -v gvsn 2> /dev/null || true)"
if [ -z "$GVSN_BIN" ] && [ -x "$INSTALL_DIR/gvsn" ]; then
    GVSN_BIN="$INSTALL_DIR/gvsn"
fi

# -- Profiles that gvsn setup can write to ---------------------------------------
PROFILES="$HOME/.bashrc $HOME/.zshrc $HOME/.config/fish/config.fish $HOME/.profile $HOME/.zprofile"

FOUND_PROFILES=""
for p in $PROFILES; do
    if [ -f "$p" ] && grep -q '^# gvsn ' "$p" 2> /dev/null; then
        FOUND_PROFILES="$FOUND_PROFILES $p"
    fi
done

VERSION_COUNT=0
if [ -d "$GVSN_DIR_PATH/versions" ]; then
    VERSION_COUNT="$(find "$GVSN_DIR_PATH/versions" -mindepth 1 -maxdepth 1 -type d 2> /dev/null | wc -l | tr -d ' ')"
fi

# -- Print removal plan ----------------------------------------------------------
printf "  ${C_BOLD}This will permanently remove:${C_RESET}\n\n"
if [ -d "$GVSN_DIR_PATH" ]; then
    plural="s"
    [ "$VERSION_COUNT" = "1" ] && plural=""
    printf "  ${C_CYAN}->${C_RESET} %s (%s installed version%s, plus cache/tmp)\n" "$GVSN_DIR_PATH" "$VERSION_COUNT" "$plural"
else
    printf "  ${C_CYAN}->${C_RESET} %s (not found)\n" "$GVSN_DIR_PATH"
fi
if [ -n "$GVSN_BIN" ]; then
    printf "  ${C_CYAN}->${C_RESET} %s\n" "$GVSN_BIN"
else
    printf "  ${C_CYAN}->${C_RESET} gvsn binary (not found in PATH or %s)\n" "$INSTALL_DIR"
fi
if [ -n "$FOUND_PROFILES" ]; then
    for p in $FOUND_PROFILES; do
        printf "  ${C_CYAN}->${C_RESET} %s (gvsn lines removed)\n" "$p"
    done
else
    printf "  ${C_CYAN}->${C_RESET} no gvsn entries found in any shell profile\n"
fi
printf "  ${C_CYAN}->${C_RESET} any leftover temp files from an interrupted install/upgrade\n"
printf "\n"

if [ "$DRY_RUN" = "1" ]; then
    printf "  ${C_YELLOW}Dry run - nothing was removed.${C_RESET}\n\n"
    exit 0
fi

# -- Confirm ---------------------------------------------------------------------
if [ "$FORCE" != "1" ]; then
    if [ -r /dev/tty ]; then
        printf "  Type ${C_BOLD}yes${C_RESET} to confirm: "
        read -r REPLY < /dev/tty
    else
        die "No interactive terminal to confirm on. Re-run with --force (or GVSN_UNINSTALL_FORCE=1) to proceed non-interactively."
    fi
    case "$(printf '%s' "$REPLY" | tr '[:upper:]' '[:lower:]')" in
        y | yes) ;;
        *) die "Aborted." ;;
    esac
    printf "\n"
fi

# -- Remove data directory -----------------------------------------------------
if [ -d "$GVSN_DIR_PATH" ]; then
    rm -rf "$GVSN_DIR_PATH"
    ok "Removed $GVSN_DIR_PATH"
fi

# -- Clean shell profiles ---------------------------------------------------------
for p in $PROFILES; do
    if strip_gvsn_blocks "$p"; then
        ok "Cleaned $p"
    fi
done

# -- Sweep leftover temp files from interrupted installs/upgrades -----------------
TMP_BASE="${TMPDIR:-/tmp}"
for pattern in gvsn-install-\* gvsn-upgrade-\* gvsn-checksums-\*; do
    for f in "$TMP_BASE"/$pattern; do
        [ -e "$f" ] || continue
        rm -rf "$f"
    done
done
for f in "$INSTALL_DIR"/.gvsn-upgrade-*; do
    [ -e "$f" ] || continue
    rm -f "$f"
done

# -- Remove the binary (last, so earlier steps still had it available) -----------
if [ -n "$GVSN_BIN" ] && [ -f "$GVSN_BIN" ]; then
    rm -f "$GVSN_BIN"
    ok "Removed $GVSN_BIN"
fi
if [ -f "$INSTALL_DIR/gvsn" ] && [ "$INSTALL_DIR/gvsn" != "$GVSN_BIN" ]; then
    rm -f "$INSTALL_DIR/gvsn"
    ok "Removed $INSTALL_DIR/gvsn"
fi

printf "\n  ${C_GREEN}${C_BOLD}gvsn has been completely removed.${C_RESET}\n"
printf "  Open a new terminal for the profile changes to take effect.\n\n"
printf "  ${C_YELLOW}Note:${C_RESET} if you saved shell completions manually (gvsn completions ...),\n"
printf "  remove that file yourself - gvsn does not track where it was written.\n\n"
