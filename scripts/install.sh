#!/bin/sh
# install.sh — Install HADAL from a release archive.
#
# Requires:
#   HADAL_DOWNLOAD_BASE  Base URL of release archives (e.g. https://github.com/OWNER/repo/releases/download)
#   HADAL_VERSION        Version to install (default: 0.1.0)
#
# Options:
#   --prefix DIR         Installation directory (default: $HOME/.local/bin)

set -eu

PREFIX="$HOME/.local/bin"
HADAL_VERSION="${HADAL_VERSION:-0.1.0}"

# Parse arguments
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --version)
            HADAL_VERSION="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--prefix DIR] [--version VERSION]"
            echo ""
            echo "Requires environment variables:"
            echo "  HADAL_DOWNLOAD_BASE  Base URL of release archives"
            echo "  HADAL_VERSION        Version to install (default: 0.1.0)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 2
            ;;
    esac
done

# Detect platform
UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"

case "$UNAME_S" in
    Linux)
        OS="linux"
        ;;
    Darwin)
        OS="darwin"
        ;;
    *)
        echo "ERROR: Unsupported OS: $UNAME_S" >&2
        echo "Supported: Linux, macOS" >&2
        exit 1
        ;;
esac

case "$UNAME_M" in
    x86_64)
        ARCH="x86_64"
        ;;
    arm64|aarch64)
        ARCH="aarch64"
        ;;
    *)
        echo "ERROR: Unsupported architecture: $UNAME_M" >&2
        echo "Supported: x86_64, arm64/aarch64" >&2
        exit 1
        ;;
esac

# Release archives are named with canonical Rust target triples (plan 5.10):
#   linux  -> x86_64-unknown-linux-musl
#   darwin -> x86_64-apple-darwin / aarch64-apple-darwin
case "${ARCH}-${OS}" in
    x86_64-linux)     TARGET="x86_64-unknown-linux-musl" ;;
    x86_64-darwin)    TARGET="x86_64-apple-darwin" ;;
    aarch64-darwin)   TARGET="aarch64-apple-darwin" ;;
    *)
        echo "ERROR: No release artifact for ${ARCH}-${OS}" >&2
        exit 1
        ;;
esac
BINARY_NAME="hadal"

# Check required environment
if [ -z "${HADAL_DOWNLOAD_BASE:-}" ]; then
    echo "ERROR: HADAL_DOWNLOAD_BASE is not set." >&2
    echo "Set this to the base URL of your release archives." >&2
    exit 1
fi

ARCHIVE_NAME="hadal-${HADAL_VERSION}-${TARGET}.tar.gz"
CHECKSUM_NAME="hadal-${HADAL_VERSION}-${TARGET}.sha256"
ARCHIVE_URL="${HADAL_DOWNLOAD_BASE%/}/${ARCHIVE_NAME}"
CHECKSUM_URL="${HADAL_DOWNLOAD_BASE%/}/${CHECKSUM_NAME}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading $ARCHIVE_NAME..."
if command -v curl >/dev/null 2>&1; then
    curl -fSL -o "$TMPDIR/$ARCHIVE_NAME" "$ARCHIVE_URL"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$TMPDIR/$ARCHIVE_NAME" "$ARCHIVE_URL"
else
    echo "ERROR: curl or wget required" >&2
    exit 1
fi

echo "Downloading $CHECKSUM_NAME..."
if command -v curl >/dev/null 2>&1; then
    curl -fSL -o "$TMPDIR/$CHECKSUM_NAME" "$CHECKSUM_URL"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$TMPDIR/$CHECKSUM_NAME" "$CHECKSUM_URL"
else
    echo "ERROR: curl or wget required" >&2
    exit 1
fi

# Verify checksum
echo "Verifying checksum..."
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$TMPDIR" && sha256sum -c "$CHECKSUM_NAME")
elif command -v shasum >/dev/null 2>&1; then
    (cd "$TMPDIR" && shasum -a 256 -c "$CHECKSUM_NAME")
else
    echo "ERROR: sha256sum or shasum required" >&2
    exit 1
fi

# Extract and install
echo "Extracting and installing..."
mkdir -p "$TMPDIR/extract"
tar -xzf "$TMPDIR/$ARCHIVE_NAME" -C "$TMPDIR/extract"

mkdir -p "$PREFIX"
cp "$TMPDIR/extract/hadal-${HADAL_VERSION}-${TARGET}/${BINARY_NAME}" "$PREFIX/"
chmod +x "$PREFIX/$BINARY_NAME"

echo "Installed: $PREFIX/$BINARY_NAME"
echo ""
echo "If not already in your PATH, add:"
echo "  export PATH=\"$PREFIX:\$PATH\""
echo ""
echo "Run '$BINARY_NAME --version' to verify."
