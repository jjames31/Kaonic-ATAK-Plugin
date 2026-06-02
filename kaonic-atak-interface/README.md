# Kaonic ATAK Interface

Stage 0 is a standalone plugin base that intentionally reproduces the existing `kaonic-atak-bridge` behavior without adding UI, filtering, configuration, or reliability changes.

The baseline bridges the current ATAK multicast paths through Reticulum over the Kaonic radio transport:

```text
239.2.3.1:6969
224.10.10.1:17012
```

The installer package must include:

```text
kaonic-plugin.toml
kaonic-atak-interface.service
kaonic-atak-interface
kaonic-atak-interface.sha256
```

Build and package example:

```bash
TARGET=armv7-unknown-linux-musleabihf
cross build --release -p kaonic-atak-interface --target "$TARGET"

STAGING=deploy/kaonic-atak-interface/staging
mkdir -p "$STAGING"
cp kaonic-atak-interface/kaonic-plugin.toml "$STAGING/kaonic-plugin.toml"
cp kaonic-atak-interface/kaonic-atak-interface.service "$STAGING/kaonic-atak-interface.service"
cp "target/$TARGET/release/kaonic-atak-interface" "$STAGING/kaonic-atak-interface"
sha256sum "$STAGING/kaonic-atak-interface" | awk '{print $1}' > "$STAGING/kaonic-atak-interface.sha256"
(cd "$STAGING" && zip -r "../kaonic-atak-interface.zip" .)
```
