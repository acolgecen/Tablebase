#!/usr/bin/env bash
#
# Tablebase — build and sign the macOS app.
#
# Produces a signed .app in <repo>/dist/. The app is signed with ad-hoc
# ("default") credentials unless a Developer ID is provided. The app is NOT
# notarized.
#
# ── Quick start ────────────────────────────────────────────────────────────
#   build/build.sh                                # ad-hoc signed build
#   build/build.sh --no-sign                      # local unsigned build
#   build/build.sh --icon path/to/icon.png        # also regenerate app icon
#
# ── Performance ────────────────────────────────────────────────────────────
#   Uses sccache (if installed) to cache Rust + bundled-DuckDB compilation, so
#   only first-ever builds compile the full ~800-crate tree. Install once with
#   `brew install sccache`. Disable with NO_SCCACHE=1.
#
# ── Config (env vars) ──────────────────────────────────────────────────────
#   SIGNING_IDENTITY   defaults to "-" (ad-hoc). For a distributable build set
#                      "Developer ID Application: Your Name (TEAMID)"
#                      (see: security find-identity -v -p codesigning)
#   NO_SCCACHE         set to skip the sccache compiler cache
#
# ── Flags ──────────────────────────────────────────────────────────────────
#   --icon <png>     regenerate the (RGBA) app icon set via `cargo tauri icon`.
#                    Omit it to reuse the committed src-tauri/icons/.
#   --no-sign        skip code signing
#   --skip-build     reuse the existing bundle (re-package/sign only)
#   -h | --help      show this help
#
set -euo pipefail

# --- locate repo paths (script is in <repo>/build) -------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
ICONS_DIR="$SRC_TAURI/icons"
DIST="$ROOT/dist"

# --- defaults / env --------------------------------------------------------
# Default to ad-hoc ("-") signing credentials; override with a Developer ID
# via SIGNING_IDENTITY for a distributable, Gatekeeper-friendly build.
SIGNING_IDENTITY="${SIGNING_IDENTITY:--}"
ENTITLEMENTS="$SCRIPT_DIR/entitlements.plist"

# Icon regeneration is opt-in: the committed icon.icns is the source of truth.
# Pass --icon <png> only when you actually want to replace it.
ICON=""
DO_SIGN=1; DO_BUILD=1

# --- pretty logging --------------------------------------------------------
bold=$(tput bold 2>/dev/null || true); dim=$(tput dim 2>/dev/null || true)
red=$(tput setaf 1 2>/dev/null || true); grn=$(tput setaf 2 2>/dev/null || true)
ylw=$(tput setaf 3 2>/dev/null || true); rst=$(tput sgr0 2>/dev/null || true)
log()  { echo "${bold}${grn}▸${rst} $*"; }
warn() { echo "${bold}${ylw}!${rst} $*"; }
die()  { echo "${bold}${red}✗ $*${rst}" >&2; exit 1; }

usage() { sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

# --- args ------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --icon)        ICON="$2"; shift 2;;
    --no-sign)     DO_SIGN=0; shift;;
    --skip-build)  DO_BUILD=0; shift;;
    -h|--help)     usage; exit 0;;
    *) die "Unknown argument: $1 (try --help)";;
  esac
done

[[ "$(uname)" == "Darwin" ]] || die "This script must run on macOS."

# --- preflight: required tooling -------------------------------------------
command -v cargo >/dev/null || die "cargo not found — install Rust: https://rustup.rs"
cargo tauri --version >/dev/null 2>&1 || die \
  "tauri-cli not found. Install it once with: cargo install tauri-cli --locked"

if [[ $DO_SIGN -eq 1 && "$SIGNING_IDENTITY" == "-" ]]; then
  warn "No SIGNING_IDENTITY set — signing ad-hoc ('-'); Gatekeeper will warn on other machines."
fi

# --- compiler cache: reuse compiled crates across cleans / checkouts / CI ----
# Without this, a `cargo clean` or fresh checkout recompiles all ~800 crates
# AND the bundled DuckDB C++ amalgamation from scratch. sccache caches both, so
# only first-ever builds pay the full cost. Opt out with NO_SCCACHE=1.
if [[ -z "${NO_SCCACHE:-}" ]] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache         # caches Rust crate compilation
  export CC="${CC:-sccache cc}"        # caches the bundled DuckDB C/C++ build
  export CXX="${CXX:-sccache c++}"
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
  log "Compiler cache: sccache enabled ($(sccache --version | awk '{print $2}'))"
else
  [[ -n "${NO_SCCACHE:-}" ]] || warn "sccache not found — clean builds recompile everything. Install: brew install sccache"
fi

# --- 1. icon preflight -----------------------------------------------------
# Tauri's generate_context! macro hard-requires an RGBA icon.png and panics at
# the END of compilation otherwise — wasting a full ~800-crate build. Validate
# (and regenerate if needed) up front so a bad icon fails in seconds, not after
# minutes of compiling. `cargo tauri icon` always emits RGBA outputs.
icon_is_rgba() {
  [[ "$(sips -g hasAlpha "$1" 2>/dev/null | awk '/hasAlpha/{print $2}')" == "yes" ]]
}

ICON_PNG="$ICONS_DIR/icon.png"
if [[ -n "$ICON" ]]; then
  log "Regenerating app icon set from $(basename "$ICON")"
  [[ -f "$ICON" ]] || die "Icon source not found: $ICON"
  ( cd "$SRC_TAURI" && cargo tauri icon "$ICON" )
elif [[ ! -f "$ICON_PNG" ]] || ! icon_is_rgba "$ICON_PNG"; then
  warn "icon.png is missing or not RGBA — regenerating (Tauri requires RGBA)."
  [[ -f "$ICON_PNG" ]] || die "No $ICON_PNG to regenerate from. Pass a source with --icon."
  ( cd "$SRC_TAURI" && cargo tauri icon "$ICON_PNG" )
else
  log "Icon OK (RGBA): ${dim}$ICON_PNG${rst}"
fi

# --- 2. build the release bundle -------------------------------------------
if [[ $DO_BUILD -eq 1 ]]; then
  log "Building release bundle (cargo tauri build)…"
  if [[ $DO_SIGN -eq 1 ]]; then
    # Tauri signs the app during bundling when this is set.
    export APPLE_SIGNING_IDENTITY="$SIGNING_IDENTITY"
  fi
  ( cd "$SRC_TAURI" && cargo tauri build )
  [[ -n "${RUSTC_WRAPPER:-}" ]] && sccache --show-stats 2>/dev/null | awk '/Compile requests|Cache hits|Cache misses/{print "  "$0}'
fi

# --- 3. locate the produced artifacts --------------------------------------
# tauri.conf.json sets bundle targets to ["app"], so only the .app is built.
APP="$(/usr/bin/find "$SRC_TAURI/target/release/bundle/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -1 || true)"
[[ -n "$APP" ]] || die "No .app found — did the build succeed?"

# --- 4. sign (defense-in-depth: Tauri already signed, we verify/re-sign) ----
if [[ $DO_SIGN -eq 1 && -n "$APP" ]]; then
  log "Signing $(basename "$APP") with hardened runtime…"
  sign_args=(--force --deep --options runtime --sign "$SIGNING_IDENTITY")
  # A secure timestamp needs a real Developer ID; skip it for ad-hoc signing.
  [[ "$SIGNING_IDENTITY" != "-" ]] && sign_args+=(--timestamp)
  [[ -f "$ENTITLEMENTS" ]] && sign_args+=(--entitlements "$ENTITLEMENTS")
  codesign "${sign_args[@]}" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
  log "Code signature verified."
fi

# --- 5. collect into dist/ --------------------------------------------------
log "Collecting artifacts into dist/"
rm -rf "$DIST"; mkdir -p "$DIST"
cp -R "$APP" "$DIST/"

echo
log "${bold}Done.${rst} Output in: ${dim}$DIST${rst}"
ls -1 "$DIST"
if [[ $DO_SIGN -eq 0 ]]; then
  warn "This build is UNSIGNED — Gatekeeper will warn on other machines."
fi
