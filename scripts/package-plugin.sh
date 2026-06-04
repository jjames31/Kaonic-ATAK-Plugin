#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:-armv7-unknown-linux-musleabihf}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN="kaonic-atak-plugin"
STAGING="$ROOT/deploy/$PLUGIN/staging"
ZIP_PATH="$ROOT/deploy/$PLUGIN/$PLUGIN.zip"
BINARY="$ROOT/target/$TARGET/release/$PLUGIN"
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct HEAD 2>/dev/null || date +%s)}"

if [[ ! -x "$BINARY" ]]; then
    echo "missing built binary: $BINARY" >&2
    echo "run: cross build --release -p $PLUGIN --target $TARGET" >&2
    exit 1
fi

rm -rf "$STAGING"
mkdir -p "$STAGING"

install -m 0644 "$ROOT/$PLUGIN/kaonic-plugin.toml" "$STAGING/kaonic-plugin.toml"
install -m 0644 "$ROOT/$PLUGIN/$PLUGIN.service" "$STAGING/$PLUGIN.service"
install -m 0755 "$BINARY" "$STAGING/$PLUGIN"
sha256sum "$STAGING/$PLUGIN" | awk '{print $1}' > "$STAGING/$PLUGIN.sha256"

find "$STAGING" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
rm -f "$ZIP_PATH"
(cd "$STAGING" && find . -type f | LC_ALL=C sort | sed 's#^\./##' | zip -X -q "$ZIP_PATH" -@)
echo "$ZIP_PATH"
