#!/bin/sh
# gvm (Go Version Manager) -- Linux and macOS uninstaller
#
# Removes every trace of gvm from the system:
#   - The data directory (~/.gvm by default): all installed Go versions,
#     the version/current pointers, the lock file, and the tmp/ staging area.
#   - The gvm binary itself.
#   - Every gvm-managed block from your shell profiles (bash, zsh, fish) and
#     login profiles (~/.profile, ~/.zprofile), wherever they are found -
#     not just the currently detected shell.
#   - Leftover temp files from an install/upgrade that was interrupted
#     mid-way (these are not cleaned up automatically if the process is
#     killed or crashes).
#
# Usage (one-liner):
#   curl -fsSL https://raw.githubusercontent.com/jhonsferg/gvm-rs/main/install/uninstall.sh | sh
#
# Pass flags through when piping by using `sh -s --`:
#   curl -fsSL .../uninstall.sh | sh -s -- --force
#   curl -fsSL .../uninstall.sh | sh -s -- --dry-run
#
# Or, since a piped script can't prompt interactively without a real
# terminal, set these before piping instead:
#   GVM_UNINSTALL_FORCE=1 curl -fsSL .../uninstall.sh | sh
#   GVM_UNINSTALL_DRY_RUN=1 curl -fsSL .../uninstall.sh | sh
#
# Customise the locations to clean (only needed if you used these at
# install time):
#   GVM_DIR=$HOME/.gvm GVM_INSTALL_DIR=$HOME/.local/bin sh uninstall.sh

set -eu

GVM_DIR_PATH="${GVM_DIR:-$HOME/.gvm}"
INSTALL_DIR="${GVM_INSTALL_DIR:-$HOME/.local/bin}"

FORCE=0
[ "${GVM_UNINSTALL_FORCE:-}" = "1" ] && FORCE=1
DRY_RUN=0
[ "${GVM_UNINSTALL_DRY_RUN:-}" = "1" ] && DRY_RUN=1
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

printf "\n  ${C_BOLD}${C_RED}gvm${C_RESET}${C_BOLD} -- uninstaller${C_RESET}\n\n"

# -- Strip gvm-managed blocks from a profile file -------------------------------
#
# Mirrors the block model used by gvm's own profile.rs: a gvm block starts at
# one of the recognised marker lines and ends at the next blank line (or EOF).
# Only gvm-managed content is touched; everything else in the file - and the
# file itself if gvm never touched it - is left exactly as it was.
#
# Returns success (0) if the file was changed, failure (1) otherwise.
strip_gvm_blocks() {
    file="$1"
    [ -f "$file" ] || return 1
    tmp="$(mktemp)"
    awk '
        /^# gvm init$/ || /^# gvm wrapper$/ || /^# gvm path$/ || /^# gvm: binary location$/ {
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
GVM_BIN="$(command -v gvm 2> /dev/null || true)"
if [ -z "$GVM_BIN" ] && [ -x "$INSTALL_DIR/gvm" ]; then
    GVM_BIN="$INSTALL_DIR/gvm"
fi

# -- Profiles that gvm setup can write to ---------------------------------------
PROFILES="$HOME/.bashrc $HOME/.zshrc $HOME/.config/fish/config.fish $HOME/.profile $HOME/.zprofile"

FOUND_PROFILES=""
for p in $PROFILES; do
    if [ -f "$p" ] && grep -q '^# gvm ' "$p" 2> /dev/null; then
        FOUND_PROFILES="$FOUND_PROFILES $p"
    fi
done

VERSION_COUNT=0
if [ -d "$GVM_DIR_PATH/versions" ]; then
    VERSION_COUNT="$(find "$GVM_DIR_PATH/versions" -mindepth 1 -maxdepth 1 -type d 2> /dev/null | wc -l | tr -d ' ')"
fi

# -- Print removal plan ----------------------------------------------------------
printf "  ${C_BOLD}This will permanently remove:${C_RESET}\n\n"
if [ -d "$GVM_DIR_PATH" ]; then
    plural="s"
    [ "$VERSION_COUNT" = "1" ] && plural=""
    printf "  ${C_CYAN}->${C_RESET} %s (%s installed version%s, plus cache/tmp)\n" "$GVM_DIR_PATH" "$VERSION_COUNT" "$plural"
else
    printf "  ${C_CYAN}->${C_RESET} %s (not found)\n" "$GVM_DIR_PATH"
fi
if [ -n "$GVM_BIN" ]; then
    printf "  ${C_CYAN}->${C_RESET} %s\n" "$GVM_BIN"
else
    printf "  ${C_CYAN}->${C_RESET} gvm binary (not found in PATH or %s)\n" "$INSTALL_DIR"
fi
if [ -n "$FOUND_PROFILES" ]; then
    for p in $FOUND_PROFILES; do
        printf "  ${C_CYAN}->${C_RESET} %s (gvm lines removed)\n" "$p"
    done
else
    printf "  ${C_CYAN}->${C_RESET} no gvm entries found in any shell profile\n"
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
        die "No interactive terminal to confirm on. Re-run with --force (or GVM_UNINSTALL_FORCE=1) to proceed non-interactively."
    fi
    case "$(printf '%s' "$REPLY" | tr '[:upper:]' '[:lower:]')" in
        y | yes) ;;
        *) die "Aborted." ;;
    esac
    printf "\n"
fi

# -- Remove data directory -----------------------------------------------------
if [ -d "$GVM_DIR_PATH" ]; then
    rm -rf "$GVM_DIR_PATH"
    ok "Removed $GVM_DIR_PATH"
fi

# -- Clean shell profiles ---------------------------------------------------------
for p in $PROFILES; do
    if strip_gvm_blocks "$p"; then
        ok "Cleaned $p"
    fi
done

# -- Sweep leftover temp files from interrupted installs/upgrades -----------------
TMP_BASE="${TMPDIR:-/tmp}"
for pattern in gvm-install-\* gvm-upgrade-\* gvm-checksums-\*; do
    for f in "$TMP_BASE"/$pattern; do
        [ -e "$f" ] || continue
        rm -rf "$f"
    done
done
for f in "$INSTALL_DIR"/.gvm-upgrade-*; do
    [ -e "$f" ] || continue
    rm -f "$f"
done

# -- Remove the binary (last, so earlier steps still had it available) -----------
if [ -n "$GVM_BIN" ] && [ -f "$GVM_BIN" ]; then
    rm -f "$GVM_BIN"
    ok "Removed $GVM_BIN"
fi
if [ -f "$INSTALL_DIR/gvm" ] && [ "$INSTALL_DIR/gvm" != "$GVM_BIN" ]; then
    rm -f "$INSTALL_DIR/gvm"
    ok "Removed $INSTALL_DIR/gvm"
fi

printf "\n  ${C_GREEN}${C_BOLD}gvm has been completely removed.${C_RESET}\n"
printf "  Open a new terminal for the profile changes to take effect.\n\n"
printf "  ${C_YELLOW}Note:${C_RESET} if you saved shell completions manually (gvm completions ...),\n"
printf "  remove that file yourself - gvm does not track where it was written.\n\n"
