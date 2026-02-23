#!/usr/bin/env bash
set -euo pipefail

# Tide DMG Installer Builder (with codesign + notarization)
#
# Prerequisites:
#   1. "Developer ID Application" certificate in Keychain
#   2. Store notarization credentials (one-time):
#      xcrun notarytool store-credentials "tide" \
#        --apple-id "your@email.com" \
#        --team-id "D86BXTY5VR" \
#        --password "app-specific-password"
#
# Usage:
#   ./scripts/build-dmg.sh              # full: build + sign + notarize + DMG
#   ./scripts/build-dmg.sh --skip-build # sign + notarize + DMG from existing .app
#   ./scripts/build-dmg.sh --no-notarize # build + sign + DMG (skip notarization)

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SKIP_BUILD=false
NO_NOTARIZE=false
SIGNING_IDENTITY="Developer ID Application: Jacob Park (D86BXTY5VR)"
NOTARIZE_PROFILE="tide"

for arg in "$@"; do
    case "$arg" in
        --skip-build)   SKIP_BUILD=true ;;
        --no-notarize)  NO_NOTARIZE=true ;;
        --help|-h)
            echo "Usage: $0 [--skip-build] [--no-notarize]"
            echo "  (default)        build + codesign + notarize + DMG"
            echo "  --skip-build     use existing .app bundle"
            echo "  --no-notarize    skip Apple notarization"
            exit 0
            ;;
        *) echo "Unknown option: $arg"; exit 1 ;;
    esac
done

# Paths
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/target/release/bundle/osx/Tide.app"
ENTITLEMENTS="$ROOT_DIR/scripts/entitlements.plist"
VERSION=$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
DMG_NAME="Tide-${VERSION}.dmg"
DMG_OUTPUT="$ROOT_DIR/target/release/$DMG_NAME"
DMG_TEMP="$ROOT_DIR/target/release/dmg-staging"

step() {
    local label="$1"
    shift
    printf "${CYAN}[%s]${NC} %s\n" "$(date +%H:%M:%S)" "$label"
    local start
    start=$(date +%s)
    if "$@"; then
        local elapsed=$(( $(date +%s) - start ))
        printf "${GREEN}  -> done${NC} (%ds)\n\n" "$elapsed"
    else
        local elapsed=$(( $(date +%s) - start ))
        printf "${RED}  -> FAILED${NC} (%ds)\n" "$elapsed"
        exit 1
    fi
}

echo ""
printf "${YELLOW}Tide DMG Installer Builder (v%s)${NC}\n\n" "$VERSION"

# ── Step 1: Build .app bundle ──────────────────────────────────────
if [ "$SKIP_BUILD" = false ]; then
    step "cargo bundle (release)" cargo bundle --release -p tide-app
fi

if [ ! -d "$APP_BUNDLE" ]; then
    printf "${RED}Error: Tide.app not found at %s${NC}\n" "$APP_BUNDLE"
    printf "Run without --skip-build to create it first.\n"
    exit 1
fi

# ── Step 2: Codesign the .app bundle ──────────────────────────────
# Sign with hardened runtime (required for notarization)
step "codesign Tide.app" codesign --force --deep --options runtime \
    --sign "$SIGNING_IDENTITY" \
    --entitlements "$ENTITLEMENTS" \
    --timestamp \
    "$APP_BUNDLE"

step "verify codesign" codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"

# ── Step 3: Create DMG ────────────────────────────────────────────
rm -rf "$DMG_TEMP"
rm -f "$DMG_OUTPUT"

step "prepare DMG contents" bash -c "
    mkdir -p '$DMG_TEMP'
    cp -R '$APP_BUNDLE' '$DMG_TEMP/Tide.app'
    ln -s /Applications '$DMG_TEMP/Applications'
"

step "create DMG" hdiutil create \
    -volname "Tide" \
    -srcfolder "$DMG_TEMP" \
    -ov \
    -format UDZO \
    "$DMG_OUTPUT"

rm -rf "$DMG_TEMP"

# Sign the DMG itself
step "codesign DMG" codesign --force \
    --sign "$SIGNING_IDENTITY" \
    --timestamp \
    "$DMG_OUTPUT"

# ── Step 4: Notarize ─────────────────────────────────────────────
if [ "$NO_NOTARIZE" = false ]; then
    step "submit for notarization" xcrun notarytool submit "$DMG_OUTPUT" \
        --keychain-profile "$NOTARIZE_PROFILE" \
        --wait

    step "staple notarization ticket" xcrun stapler staple "$DMG_OUTPUT"
else
    printf "${YELLOW}Skipping notarization (--no-notarize)${NC}\n\n"
fi

# ── Done ──────────────────────────────────────────────────────────
printf "${GREEN}DMG created:${NC} %s\n" "$DMG_OUTPUT"
printf "${GREEN}Size:${NC} %s\n" "$(du -h "$DMG_OUTPUT" | cut -f1)"

if [ "$NO_NOTARIZE" = false ]; then
    printf "${GREEN}Status:${NC} signed + notarized (Gatekeeper will pass)\n"
else
    printf "${YELLOW}Status:${NC} signed only (not notarized)\n"
fi
