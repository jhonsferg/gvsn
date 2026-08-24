#!/bin/sh
# gvsn (Go Version Manager) -- Linux and macOS installer
#
# Usage (one-liner):
#   curl -fsSL https://raw.githubusercontent.com/jhonsferg/gvsn/main/install/install.sh | sh
#
# Customise with environment variables before piping:
#   GVSN_INSTALL_DIR=$HOME/.local/bin GVSN_VERSION=v1.0.0 curl -fsSL ... | sh

set -eu

REPO="${GVSN_REPO:-jhonsferg/gvsn}"
INSTALL_DIR="${GVSN_INSTALL_DIR:-$HOME/.local/bin}"
GVSN_VERSION="${GVSN_VERSION:-latest}"

# Override base URLs for local/offline testing:
#   GVSN_TEST_API_BASE=http://localhost:8765
#   GVSN_TEST_DL_BASE=http://localhost:8765
API_BASE="${GVSN_TEST_API_BASE:-https://api.github.com}"
DL_BASE="${GVSN_TEST_DL_BASE:-https://github.com}"

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

printf "\n  ${C_BOLD}${C_CYAN}gvsn${C_RESET}${C_BOLD} -- Go Version Manager${C_RESET} installer\n\n"

# -- 1. Check required tools ---------------------------------------------------
need() {
    command -v "$1" > /dev/null 2>&1 || die "'$1' is required but not installed."
}
need curl
need tar

# Computes the SHA-256 digest of $1, preferring GNU coreutils' sha256sum and
# falling back to macOS/BSD's shasum.
sha256_of() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "Neither 'sha256sum' nor 'shasum' is available to verify the download."
    fi
}

# -- 2. Detect OS and architecture ---------------------------------------------
OS="$(uname -s 2>/dev/null || echo unknown)"
case "$OS" in
    Linux)  PLATFORM="linux"  ;;
    Darwin) PLATFORM="darwin" ;;
    *)      die "Unsupported OS: $OS  (only Linux and macOS are supported)" ;;
esac

# Termux (Android) ships a Linux kernel but musl binaries cannot use Android's
# DNS resolver. Detect Termux and switch to the dedicated android binary which
# is linked against bionic and uses Android DNS natively.
if [ "$PLATFORM" = "linux" ] && { [ -n "${TERMUX_VERSION:-}" ] || [ -d "/data/data/com.termux" ]; }; then
    PLATFORM="android"
fi

MACHINE="$(uname -m 2>/dev/null || echo unknown)"
case "$MACHINE" in
    x86_64 | amd64)             ARCH="x86_64"  ;;
    aarch64 | arm64 | armv8*)   ARCH="aarch64" ;;
    armv7* | armhf | armv6l)    ARCH="armv7"   ;;
    i686 | i386)                ARCH="386"     ;;
    riscv64)                    ARCH="riscv64" ;;
    s390x)                      ARCH="s390x"   ;;
    ppc64le)                    ARCH="ppc64le" ;;
    *)                          die "Unsupported architecture: $MACHINE" ;;
esac

if [ "$PLATFORM" = "android" ] && [ "$ARCH" != "aarch64" ]; then
    die "Termux is only supported on aarch64 devices. Detected arch: $ARCH"
fi

step "Detected platform: $PLATFORM-$ARCH"

# -- 3. Resolve version --------------------------------------------------------
if [ "$GVSN_VERSION" = "latest" ]; then
    step "Fetching latest release from $API_BASE..."
    API_RESPONSE="$(curl -sSL --retry 3 --max-time 30 -w '\n%{http_code}' "$API_BASE/repos/$REPO/releases/latest")" \
        || die "Could not reach $API_BASE. Check your internet connection."
    API_STATUS="$(printf '%s' "$API_RESPONSE" | tail -n 1)"
    API_BODY="$(printf '%s' "$API_RESPONSE" | sed '$d')"
    if [ "$API_STATUS" = "403" ]; then
        die "GitHub API rate limit exceeded (HTTP 403). Try again later, or set GVSN_VERSION explicitly."
    fi
    if [ "$API_STATUS" != "200" ]; then
        die "Failed to resolve the latest version (HTTP $API_STATUS)."
    fi
    GVSN_VERSION="$(printf '%s' "$API_BODY" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
    [ -n "$GVSN_VERSION" ] || die "Failed to parse the latest version from the GitHub API response."
fi

step "Installing gvsn $GVSN_VERSION"

# -- 4. Download archive -------------------------------------------------------
ARCHIVE="gvsn_${PLATFORM}_${ARCH}.tar.gz"
URL="$DL_BASE/$REPO/releases/download/$GVSN_VERSION/$ARCHIVE"
CHECKSUMS_URL="$DL_BASE/$REPO/releases/download/$GVSN_VERSION/checksums.txt"
TMP_FILE="$(mktemp "${TMPDIR:-/tmp}/gvsn-install.XXXXXX")"
CHECKSUMS_FILE="$(mktemp "${TMPDIR:-/tmp}/gvsn-checksums.XXXXXX")"

trap 'rm -f "$TMP_FILE" "$CHECKSUMS_FILE"' EXIT INT TERM

step "Downloading $ARCHIVE..."
if ! curl -fSL --retry 3 --retry-delay 2 --max-time 300 --progress-bar "$URL" -o "$TMP_FILE"; then
    die "Download failed.\n  URL: $URL\n  Check that release $GVSN_VERSION exists."
fi

step "Verifying checksum..."
if ! curl -fsSL --retry 3 --max-time 30 "$CHECKSUMS_URL" -o "$CHECKSUMS_FILE"; then
    die "Failed to download checksums.txt for verification.\n  URL: $CHECKSUMS_URL"
fi
EXPECTED_SHA="$(awk -v f="$ARCHIVE" '$2 == f {print $1}' "$CHECKSUMS_FILE")"
[ -n "$EXPECTED_SHA" ] || die "No checksum entry found for $ARCHIVE in checksums.txt"
ACTUAL_SHA="$(sha256_of "$TMP_FILE")"
if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
    die "Checksum mismatch for $ARCHIVE!\n  expected: $EXPECTED_SHA\n  got:      $ACTUAL_SHA"
fi
ok "Checksum verified"

# -- 5. Extract and install binary ---------------------------------------------
mkdir -p "$INSTALL_DIR"
step "Extracting..."
if ! tar xzf "$TMP_FILE" -C "$INSTALL_DIR" gvsn; then
    die "Extraction failed. The archive may be corrupted."
fi
chmod +x "$INSTALL_DIR/gvsn"
ok "Installed to $INSTALL_DIR/gvsn"

# -- 6. Run gvsn setup ----------------------------------------------------------
# Run setup via full path so it works even before INSTALL_DIR is on PATH.
printf "\n  ${C_BOLD}Configuring shell environment...${C_RESET}\n\n"
"$INSTALL_DIR/gvsn" setup

# -- 7. Summary ----------------------------------------------------------------
printf "\n  ${C_GREEN}${C_BOLD}gvsn $GVSN_VERSION installed and configured!${C_RESET}\n\n"
printf "  Next steps:\n\n"
printf "  1. Restart your shell (or source your profile), then install Go:\n"
printf "       ${C_CYAN}gvsn install latest${C_RESET}\n"
printf "       ${C_CYAN}gvsn use latest${C_RESET}\n\n"
printf "  Run ${C_CYAN}gvsn doctor${C_RESET} to verify the setup.\n\n"
