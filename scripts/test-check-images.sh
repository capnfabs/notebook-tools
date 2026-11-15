#!/usr/bin/env bash
# Tests for check-images.sh and the pre-commit hook, each in a throwaway repo.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"

RED_PNG='iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAACXBIWXMAAAABAAAAAQBPJcTWAAAAEElEQVR4nGP8wwACLGCSAQANBAECv1AVswAAAABJRU5ErkJggg=='
BLUE_PNG='iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAACXBIWXMAAAABAAAAAQBPJcTWAAAAEklEQVR4nGNkYPjLwMDAwgAGAAsXAQPfmWhAAAAAAElFTkSuQmCC'
# Carries a JPEG comment ("Lavc62.11.100") from the encoder that made it.
COMMENTED_JPG='/9j/4AAQSkZJRgABAgAAAQABAAD//gAQTGF2YzYyLjExLjEwMAD/2wBDAAgKCgsKCw0NDQ0NDRAPEBAQEBAQEBAQEBASEhIVFRUSEhIQEBISFBQVFRcXFxUVFRUXFxkZGR4eHBwjIyQrKzP/xABLAAEBAAAAAAAAAAAAAAAAAAAABgEBAAAAAAAAAAAAAAAAAAAABhABAAAAAAAAAAAAAAAAAAAAABEBAAAAAAAAAAAAAAAAAAAAAP/AABEIAAgACAMBIgACEQADEQD/2gAMAwEAAhEDEQA/AIsAUCX/2Q=='

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
passed=0

# A fresh repo with the script and hook, and one commit, as the current directory.
fresh_repo() {
    local dir="$root/$1"
    mkdir -p "$dir/scripts" "$dir/.githooks"
    cp "$here/scripts/check-images.sh" "$dir/scripts/"
    cp "$here/.githooks/pre-commit" "$dir/.githooks/"
    cd "$dir"
    git init -q
    git config user.name test
    git config user.email test@example.com
    git config core.hooksPath .githooks
    echo '# reviewed' >scripts/reviewed-images.txt
    git add -A
    git commit -qm init
}

image() { mkdir -p "$(dirname "$2")"; echo "$1" | base64 --decode >"$2"; }
check() { scripts/check-images.sh "$@" 2>"$root/err"; }
ok() { passed=$((passed + 1)); }
fail() { echo "FAIL: $*"; cat "$root/err" 2>/dev/null; exit 1; }
expect_error() { grep -qF -- "$1" "$root/err" || fail "stderr lacks '$1'"; }

# Nothing staged passes.
fresh_repo empty
check || fail "empty index rejected"; ok

# An unreviewed image is rejected, and approving it lets it through.
fresh_repo unreviewed
image "$RED_PNG" photos/a.png
git add photos/a.png
check && fail "unreviewed image accepted"
expect_error "'photos/a.png' hasn't been reviewed"; ok
scripts/check-images.sh --approve photos/a.png >/dev/null
check || fail "approved image rejected"; ok
git diff --cached --name-only | grep -qx scripts/reviewed-images.txt || fail "list not staged"; ok

# Non-images aren't checked; nor are deletions.
fresh_repo other-files
echo hi >notes.md
git add notes.md
check || fail "non-image rejected"; ok
image "$RED_PNG" a.png
git add a.png && scripts/check-images.sh --approve a.png >/dev/null && git commit -qm a
git rm -q a.png
check || fail "deletion rejected"; ok

# Editing an approved image loses the approval; renaming keeps it.
fresh_repo edits
image "$RED_PNG" a.png
git add a.png && scripts/check-images.sh --approve a.png >/dev/null && git commit -qm a
git mv a.png b.png
check || fail "rename lost approval"; ok
image "$BLUE_PNG" b.png
git add b.png
check && fail "edited image accepted"; ok

# Re-approving a path replaces its old entry.
scripts/check-images.sh --approve b.png >/dev/null
scripts/check-images.sh --approve b.png >/dev/null
[ "$(grep -c ' b.png$' scripts/reviewed-images.txt)" = 1 ] || fail "duplicate entries"; ok

# Location metadata is rejected even once approved, and can't be approved.
fresh_repo gps
image "$RED_PNG" a.png
exiftool -q -overwrite_original -GPSLatitude=51.75 -GPSLatitudeRef=N a.png
git add a.png
scripts/check-images.sh --approve a.png >/dev/null 2>"$root/err" && fail "approved an image with GPS"
expect_error GPSLatitude; ok
blob=$(git rev-parse :a.png)
echo "$blob a.png" >>scripts/reviewed-images.txt && git add scripts/reviewed-images.txt
check && fail "GPS accepted"
expect_error GPSLatitude
expect_error "exiftool -all="; ok

# A HEIC gets its own strip hint (`-all=` breaks it for ImageIO), which clears its metadata.
# Making one needs macOS's sips.
if command -v sips >/dev/null; then
    fresh_repo heic
    image "$RED_PNG" a.png
    sips -s format heic a.png --out a.heic >/dev/null && rm a.png
    exiftool -q -overwrite_original -GPSLatitude=51.75 -GPSLatitudeRef=N a.heic
    git add a.heic
    scripts/check-images.sh --approve a.heic >/dev/null 2>"$root/err" && fail "approved a HEIC with GPS"
    expect_error GPSLatitude
    expect_error "exiftool -gps:all= -xmp:all= -exif:all= -makernotes:all="
    grep -qF -- "exiftool -all=" "$root/err" && fail "HEIC told to use -all="
    ok
    exiftool -q -overwrite_original -gps:all= -xmp:all= -exif:all= -makernotes:all= a.heic
    git add a.heic
    scripts/check-images.sh --approve a.heic >/dev/null 2>"$root/err" || fail "stripped HEIC refused"; ok
fi

# Rotation and colour space are allowed; the rest of EXIF isn't.
fresh_repo rotation
image "$RED_PNG" a.png
exiftool -q -overwrite_original -Orientation#=3 -ColorSpace=sRGB a.png
git add a.png
scripts/check-images.sh --approve a.png >/dev/null 2>"$root/err" || fail "rotated image refused"
check || fail "rotated image rejected"; ok
exiftool -q -overwrite_original -Model='iPhone' a.png
git add a.png
scripts/check-images.sh --approve a.png >/dev/null 2>"$root/err" && fail "approved a camera model"
expect_error Model
grep -qE "\] +Orientation" "$root/err" && fail "Orientation reported"; ok

# PNG text chunks and JPEG comments are metadata too. Upper-case extensions count.
fresh_repo text
image "$RED_PNG" a.png
exiftool -q -overwrite_original -PNG:Author='Someone' a.png
image "$COMMENTED_JPG" B.JPG
git add a.png B.JPG
check --all && fail "text metadata accepted"
expect_error "Author"
expect_error "Comment"
expect_error "'B.JPG' hasn't been reviewed"; ok

# --installed fails until the hook is on.
fresh_repo installed
check --installed || fail "hook on, but --installed failed"; ok
git config --unset core.hooksPath
check --installed && fail "hook off, but --installed passed"
expect_error "git config core.hooksPath .githooks"; ok

# The hook blocks a commit with an unreviewed image.
fresh_repo hook
image "$RED_PNG" a.png
git add a.png
git commit -qm a 2>"$root/err" && fail "hook let an unreviewed image through"
expect_error "hasn't been reviewed"; ok

echo "check-images: $passed passed"
