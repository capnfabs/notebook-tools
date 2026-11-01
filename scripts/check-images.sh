#!/usr/bin/env bash
# Keeps unreviewed images, and metadata that says where, when or with what a
# photo was taken, out of commits. Run by `.githooks/pre-commit`.
# See decisions/2026-11-01-image-guardrails.md.
#
#   check-images.sh                  check images staged for commit (added or changed)
#   check-images.sh --all            check every image in the index
#   check-images.sh --approve PATH…  record PATH's staged content as reviewed by a human
#   check-images.sh --installed      fail unless the hook is on and can run (scripts/check.sh)
#
# Images are checked as staged, not as they are in the working tree. An image is
# reviewed when its git blob id is listed in scripts/reviewed-images.txt, so a
# rename keeps its approval and any edit loses it.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

LIST=scripts/reviewed-images.txt
IMAGE_RE='\.(jpe?g|png|gif|webp|heic|heif|avif|tiff?|bmp|dng)$'
# Metadata that can identify a place, person, device or time. Colour profiles,
# rotation and pixel layout are fine.
META_TAGS=(-EXIF:all -XMP:all -IPTC:all -GPS:all -MakerNotes:all -Photoshop:all
    -File:Comment -PNG:all)
# EXIF tags that pass: rotation, colour, layout and format versions (exiftool
# adds those whenever it writes EXIF). Everything else in EXIF (camera, dates,
# GPS, comments, thumbnails) fails.
EXIF_OK='Orientation|ColorSpace|XResolution|YResolution|ResolutionUnit|ExifImageWidth|ExifImageHeight|YCbCrPositioning|ExifVersion|ComponentsConfiguration|FlashpixVersion'
# XMP that passes: the toolkit stamp, and the HDR gain map iPhones attach to HEICs.
XMP_OK='\[XMP-x\] +XMPToolkit|\[XMP-HDRGainMap\] +[A-Za-z]+'
# PNG text chunks share a group with the image header, so PNG tags fail unless
# they're one of these.
PNG_OK='ImageWidth|ImageHeight|BitDepth|ColorType|Compression|Filter|Interlace|PixelsPerUnit[XY]|PixelUnits|Gamma|SRGBRendering|WhitePoint[XY]|(Red|Green|Blue)[XY]|BackgroundColor|SignificantBits|Transparency|Palette|ProfileName|ColorPrimaries|TransferCharacteristics|MatrixCoefficients|VideoFullRangeFlag|AppleDataOffsets|AnimationFrames|AnimationPlays'

if ! command -v exiftool >/dev/null; then
    echo "check-images: exiftool is needed (brew install exiftool)" >&2
    exit 2
fi

# Git never turns on hooks from a clone, so each checkout has to.
if [ "${1-}" = --installed ]; then
    if [ "$(git config --get core.hooksPath || true)" != .githooks ]; then
        echo "check-images: the pre-commit hook is off; run: git config core.hooksPath .githooks" >&2
        exit 1
    fi
    exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The tags in META_TAGS that the staged blob $1 (named like $2) carries.
metadata() {
    local file="$tmp/blob.${2##*.}"
    git cat-file blob "$1" >"$file"
    { exiftool -q -q -s -G1 -a "${META_TAGS[@]}" "$file" || true; } |
        grep -vE "^\[PNG[^]]*\] +($PNG_OK) +:" |
        grep -vE "^\[(IFD0|ExifIFD)\] +($EXIF_OK) +:" |
        grep -vE "^($XMP_OK) +:" || true
}

reviewed() {
    git show ":$LIST" 2>/dev/null | grep -v '^#' | cut -d' ' -f1 | grep -qxF "$1"
}

# Paths of images to check, one per line.
images() {
    if [ "${1-}" = --all ]; then
        git ls-files
    else
        git diff --cached --name-only --diff-filter=ACMR
    fi | grep -iE "$IMAGE_RE" || true
}

strip_hint() {
    echo "  strip it (keeps rotation and colour profile):" >&2
    echo "  exiftool -all= --icc_profile:all -tagsfromfile @ -Orientation -ColorSpace -overwrite_original '$1'" >&2
}

if [ "${1-}" = --approve ]; then
    shift
    [ $# -gt 0 ] || { echo "usage: $0 --approve PATH..." >&2; exit 2; }
    touch "$LIST"
    for path in "$@"; do
        blob=$(git rev-parse --verify --quiet ":$path") || {
            echo "check-images: '$path' isn't staged; git add it first" >&2
            exit 1
        }
        meta=$(metadata "$blob" "$path")
        if [ -n "$meta" ]; then
            echo "check-images: not approving '$path', it carries metadata:" >&2
            echo "$meta" | sed 's/^/    /' >&2
            strip_hint "$path"
            exit 1
        fi
        awk -v p="$path" 'substr($0, index($0, " ") + 1) != p' "$LIST" >"$tmp/list"
        echo "$blob $path" >>"$tmp/list"
        { grep '^#' "$tmp/list" || true; grep -v '^#' "$tmp/list" | sort -k2 || true; } >"$LIST"
        echo "approved $path ($blob)"
    done
    git add "$LIST"
    exit 0
fi

failed=0
while IFS= read -r path; do
    [ -n "$path" ] || continue
    blob=$(git rev-parse ":$path")
    if ! reviewed "$blob"; then
        echo "check-images: '$path' hasn't been reviewed." >&2
        echo "  Check it shows nothing personal (including the back of the page), then:" >&2
        echo "  scripts/check-images.sh --approve '$path'" >&2
        failed=1
    fi
    meta=$(metadata "$blob" "$path")
    if [ -n "$meta" ]; then
        echo "check-images: '$path' carries metadata:" >&2
        echo "$meta" | sed 's/^/    /' >&2
        strip_hint "$path"
        failed=1
    fi
done < <(images "${1-}")
exit $failed
