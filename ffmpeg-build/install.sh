#!/bin/bash
# Copies built FFmpeg DLLs to src-tauri/installers/dlls/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC="${SCRIPT_DIR}/dist/bin"
DST="${SCRIPT_DIR}/../src-tauri/installers/dlls"

DLLS=(
    "avcodec-61.dll"
    "avformat-61.dll"
    "avutil-59.dll"
    "avfilter-10.dll"
    "swresample-5.dll"
    "swscale-8.dll"
)

if [ ! -d "$SRC" ]; then
    echo "ERROR: dist/bin/ not found. Run build.sh first."
    exit 1
fi

echo "Copying FFmpeg DLLs to $(basename "$DST")/"
for dll in "${DLLS[@]}"; do
    if [ ! -f "${SRC}/${dll}" ]; then
        echo "  ERROR: ${dll} not found in dist/bin/"
        exit 1
    fi
    OLD_SIZE=$(stat -c%s "${DST}/${dll}" 2>/dev/null || echo "0")
    NEW_SIZE=$(stat -c%s "${SRC}/${dll}")
    cp "${SRC}/${dll}" "${DST}/"
    OLD_KB=$((OLD_SIZE / 1024))
    NEW_KB=$((NEW_SIZE / 1024))
    printf "  %-25s %6d KB -> %6d KB\n" "$dll" "$OLD_KB" "$NEW_KB"
done

echo ""
echo "Done. Verify with:"
echo "  cd \"${DST}\" && powershell ./check_deps.ps1"
