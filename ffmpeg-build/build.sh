#!/bin/bash
# =============================================================================
# Minimal FFmpeg build — FFV1 encoder/decoder only
#
# Produces drop-in replacement DLLs for the full FFmpeg libs bundled by
# GStreamer, containing ONLY the FFV1 codec (no patent-encumbered code).
#
# Must run inside MSYS2 with MSVC tools in PATH.
# See README.md for prerequisites and step-by-step instructions.
# =============================================================================

set -euo pipefail

FFMPEG_VERSION="7.1.1"
FFMPEG_TARBALL="ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_URL="https://ffmpeg.org/releases/${FFMPEG_TARBALL}"
FFMPEG_SIG_URL="${FFMPEG_URL}.asc"
FFMPEG_SRC_DIR="ffmpeg-${FFMPEG_VERSION}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"
DIST_DIR="${SCRIPT_DIR}/dist"

# FFmpeg release signing key (Michael Niedermayer)
FFMPEG_GPG_KEY="FCF986EA15E6E293A5644F10B4322F04D67658D8"

# =============================================================================
# Environment checks
# =============================================================================

echo "=== Environment checks ==="

# Check MSVC compiler
if ! command -v cl.exe &>/dev/null; then
    echo "ERROR: cl.exe not found in PATH."
    echo "Run this script from MSYS2 launched inside 'x64 Native Tools Command Prompt for VS'"
    echo "with PATH inheritance enabled:"
    echo ""
    echo "  set MSYS2_PATH_TYPE=inherit"
    echo "  C:\\msys64\\msys2_shell.cmd -defterm -here -no-start -msys2"
    exit 1
fi
echo "cl.exe: $(cl.exe 2>&1 | head -1)"

# Ensure MSVC's link.exe wins over MSYS2's /usr/bin/link.
# Prepend the MSVC bin directory to PATH so it resolves first.
MSVC_BIN_DIR="$(dirname "$(which cl.exe)")"
export PATH="${MSVC_BIN_DIR}:${PATH}"

LINK_PATH="$(which link)"
if [[ "$LINK_PATH" == /usr/* ]]; then
    echo "ERROR: 'link' still resolves to MSYS2: ${LINK_PATH}"
    echo "MSVC bin dir: ${MSVC_BIN_DIR}"
    exit 1
fi
echo "link.exe: ${LINK_PATH}"

# Check nasm (needed for x86 SIMD optimizations)
if ! command -v nasm &>/dev/null; then
    echo "ERROR: nasm not found. Install it: pacman -S nasm"
    exit 1
fi
echo "nasm:     $(nasm --version)"

# Check make
if ! command -v make &>/dev/null; then
    echo "ERROR: make not found. Install it: pacman -S make"
    exit 1
fi

echo ""

# =============================================================================
# Source acquisition
# =============================================================================

cd "$SCRIPT_DIR"

if [ ! -f "$FFMPEG_TARBALL" ]; then
    echo "=== Downloading FFmpeg ${FFMPEG_VERSION} ==="
    curl -L -o "$FFMPEG_TARBALL" "$FFMPEG_URL"
    echo ""
fi

# PGP signature verification (optional but recommended)
if command -v gpg &>/dev/null; then
    if [ ! -f "${FFMPEG_TARBALL}.asc" ]; then
        echo "=== Downloading PGP signature ==="
        curl -L -o "${FFMPEG_TARBALL}.asc" "$FFMPEG_SIG_URL"
    fi

    echo "=== Verifying PGP signature ==="
    # Import FFmpeg signing key if not already present
    if ! gpg --list-keys "$FFMPEG_GPG_KEY" &>/dev/null; then
        echo "Importing FFmpeg release signing key..."
        gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys "$FFMPEG_GPG_KEY" || \
        gpg --keyserver hkps://keys.openpgp.org --recv-keys "$FFMPEG_GPG_KEY" || \
            echo "WARNING: Could not import GPG key. Skipping signature verification."
    fi

    if gpg --list-keys "$FFMPEG_GPG_KEY" &>/dev/null; then
        if gpg --verify "${FFMPEG_TARBALL}.asc" "$FFMPEG_TARBALL" 2>&1; then
            echo "PGP signature: VALID"
        else
            echo "ERROR: PGP signature verification FAILED."
            echo "The tarball may be corrupted or tampered with."
            exit 1
        fi
    fi
    echo ""
else
    echo "NOTE: gpg not found — skipping PGP signature verification."
    echo "      Install with: pacman -S gnupg"
    echo ""
fi

# Extract
if [ ! -d "$FFMPEG_SRC_DIR" ]; then
    echo "=== Extracting ==="
    tar xf "$FFMPEG_TARBALL"
    echo ""
fi

# =============================================================================
# Clean previous build
# =============================================================================

if [ -d "$BUILD_DIR" ] || [ -d "$DIST_DIR" ]; then
    echo "=== Cleaning previous build ==="
    rm -rf "$BUILD_DIR" "$DIST_DIR"
    echo ""
fi

# =============================================================================
# Configure
# =============================================================================

mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

echo "=== Configuring FFmpeg (FFV1 only) ==="
"${SCRIPT_DIR}/${FFMPEG_SRC_DIR}/configure" \
    --toolchain=msvc \
    --prefix="$DIST_DIR" \
    --enable-shared \
    --disable-static \
    --disable-programs \
    --disable-doc \
    --disable-everything \
    --enable-encoder=ffv1 \
    --enable-decoder=ffv1 \
    --disable-avdevice \
    --disable-postproc \
    --disable-network \
    --disable-autodetect \
    --extra-cflags="-MD"

echo ""

# =============================================================================
# Build & install
# =============================================================================

echo "=== Building ==="
make -r -j"$(nproc)"

echo ""
echo "=== Installing ==="
make -r install

echo ""

# =============================================================================
# Verification
# =============================================================================

echo "=== Verification ==="

DLLS_DIR="${DIST_DIR}/bin"
PASS=true

# 1. Presence check — all 6 DLLs must exist
EXPECTED_DLLS=(
    "avcodec-61.dll"
    "avformat-61.dll"
    "avutil-59.dll"
    "avfilter-10.dll"
    "swresample-5.dll"
    "swscale-8.dll"
)

for dll in "${EXPECTED_DLLS[@]}"; do
    if [ -f "${DLLS_DIR}/${dll}" ]; then
        echo "  FOUND: ${dll}"
    else
        echo "  MISSING: ${dll}"
        PASS=false
    fi
done
echo ""

# 2. String scan — no patent-encumbered codec implementations in avcodec.
#    We check for ff_*_encoder/ff_*_decoder symbols (actual codec registrations),
#    not codec ID descriptor strings which are always present in libavcodec.
if [ -f "${DLLS_DIR}/avcodec-61.dll" ]; then
    # Use PowerShell to extract strings since MSYS2 may not have 'strings'
    CODEC_IMPLS=$(powershell -Command "
        \$bytes = [System.IO.File]::ReadAllBytes('$(cygpath -w "${DLLS_DIR}/avcodec-61.dll")')
        \$text = [System.Text.Encoding]::ASCII.GetString(\$bytes)
        \$m = [regex]::Matches(\$text, 'ff_(h264|hevc|aac|mp3|mpeg[24]|ac3|eac3|amr|dts|wmv|wma|libx26[45])[a-z0-9_]*(encoder|decoder)')
        foreach (\$x in \$m) { Write-Output \$x.Value }
    " 2>/dev/null || true)
    if [ -z "$CODEC_IMPLS" ]; then
        echo "  CLEAN: No patent-encumbered codec implementations found in avcodec-61.dll"
    else
        echo "  WARNING: Found patent-encumbered codec implementation(s) in avcodec-61.dll:"
        echo "$CODEC_IMPLS" | sed 's/^/    /'
        PASS=false
    fi
fi
echo ""

# 3. Size check
if [ -f "${DLLS_DIR}/avcodec-61.dll" ]; then
    AVCODEC_SIZE=$(stat -c%s "${DLLS_DIR}/avcodec-61.dll" 2>/dev/null || stat -f%z "${DLLS_DIR}/avcodec-61.dll" 2>/dev/null)
    AVCODEC_SIZE_MB=$((AVCODEC_SIZE / 1024 / 1024))
    if [ "$AVCODEC_SIZE_MB" -ge 2 ]; then
        echo "  WARNING: avcodec-61.dll is ${AVCODEC_SIZE_MB} MB (expected < 2 MB)"
        echo "           This may indicate unwanted codecs were included."
    else
        echo "  SIZE OK: avcodec-61.dll is under 2 MB"
    fi
fi
echo ""

# =============================================================================
# Output summary
# =============================================================================

echo "=== Output DLLs ==="
TOTAL_SIZE=0
for dll in "${EXPECTED_DLLS[@]}"; do
    if [ -f "${DLLS_DIR}/${dll}" ]; then
        SIZE=$(stat -c%s "${DLLS_DIR}/${dll}" 2>/dev/null || stat -f%z "${DLLS_DIR}/${dll}" 2>/dev/null)
        SIZE_KB=$((SIZE / 1024))
        TOTAL_SIZE=$((TOTAL_SIZE + SIZE))
        printf "  %-25s %6d KB\n" "$dll" "$SIZE_KB"
    fi
done
TOTAL_KB=$((TOTAL_SIZE / 1024))
echo "  -------------------------  --------"
printf "  %-25s %6d KB\n" "TOTAL" "$TOTAL_KB"
echo ""

if [ "$PASS" = true ]; then
    echo "=== BUILD SUCCESSFUL ==="
else
    echo "=== BUILD COMPLETED WITH WARNINGS ==="
fi

# Print copy commands
REPO_DLLS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)/src-tauri/installers/dlls"
echo ""
echo "To replace the bundled DLLs, run:"
echo ""
for dll in "${EXPECTED_DLLS[@]}"; do
    echo "  cp \"${DLLS_DIR}/${dll}\" \"${REPO_DLLS_DIR}/\""
done
echo ""
echo "Then verify with:"
echo "  cd \"${REPO_DLLS_DIR}\" && powershell ./check_deps.ps1"
