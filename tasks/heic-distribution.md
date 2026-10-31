# Bundle libheif for distribution

The binary links Homebrew's libheif and its codecs (libde265 etc.) as dylibs, so it only runs where they're installed. Bundle them (or switch to ImageIO) before shipping. See `decisions/2026-10-31-image-decoding.md`.
