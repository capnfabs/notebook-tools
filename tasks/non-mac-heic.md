# HEIC on non-mac builds

For the first non-mac build (`decisions/2026-11-01-heic-through-imageio.md`): ship libheif + libde265 as separate shared libraries, built without x265, with LGPL notices and source offer. Fedora strips libde265 from its libheif for patent reasons; Windows could use WIC + HEVC extension instead. `scripts/check-linux.sh` builds and tests the libheif decoder on Linux (`decisions/2026-11-15-local-linux-ci.md`).
