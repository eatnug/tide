#!/usr/bin/env bash
set -euo pipefail

# Tide DMG Installer Builder
# Usage:
#   ./scripts/build-dmg.sh              # build .app bundle + create DMG
#   ./scripts/build-dmg.sh --skip-build # create DMG from existing .app bundle

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SKIP_BUILD=false
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=true ;;
        --help|-h)
            echo "Usage: $0 [--skip-build]"
            echo "  (default)      build release .app bundle + create DMG"
            echo "  --skip-build   create DMG from existing .app bundle"
            exit 0
            ;;
        *) echo "Unknown option: $arg"; exit 1 ;;
    esac
done

# Paths
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/target/release/bundle/osx/Tide.app"
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

# Step 1: Build .app bundle
if [ "$SKIP_BUILD" = false ]; then
    step "cargo bundle (release)" cargo bundle --release -p tide-app
fi

# Verify .app exists
if [ ! -d "$APP_BUNDLE" ]; then
    printf "${RED}Error: Tide.app not found at %s${NC}\n" "$APP_BUNDLE"
    printf "Run without --skip-build to create it first.\n"
    exit 1
fi

# Step 2: Clean previous staging
rm -rf "$DMG_TEMP"
rm -f "$DMG_OUTPUT"

# Step 3: Create staging directory with app + Applications symlink
step "prepare DMG contents" bash -c "
    mkdir -p '$DMG_TEMP'
    cp -R '$APP_BUNDLE' '$DMG_TEMP/Tide.app'
    ln -s /Applications '$DMG_TEMP/Applications'
"

# Step 4: Create DMG
step "create DMG" hdiutil create \
    -volname "Tide" \
    -srcfolder "$DMG_TEMP" \
    -ov \
    -format UDZO \
    "$DMG_OUTPUT"

# Step 5: Clean up staging
rm -rf "$DMG_TEMP"

printf "${GREEN}DMG created:${NC} %s\n" "$DMG_OUTPUT"
printf "${GREEN}Size:${NC} %s\n" "$(du -h "$DMG_OUTPUT" | cut -f1)"
