# Kaonic ATAK Plugin

Custom Kaonic-side ATAK bridge service for validated Cursor-on-Target traffic.
It uses the upstream Beechat ATAK bridge flow as a Reticulum transport reference,
but adds local safety checks before forwarding payloads.

The plugin bridges the current ATAK multicast paths through Reticulum over the
selected Kaonic radio module:

```text
239.2.3.1:6969
224.10.10.1:17012
```

## Safety Defaults

- CoT XML is parsed before forwarding local UDP into Reticulum.
- Remote Reticulum payloads are parsed before rebroadcast to ATAK multicast.
- Valid CoT events with a `point` element update in-memory location state by
  UID.
- Local ATAK traffic is bound to one selected IPv4 interface/address.
- Auto-detection only succeeds for a single safe candidate, preferring one
  `192.168.10.0/24` interface.
- Reticulum radio setup touches only the configured `--rns-module`.
- Invalid payloads are dropped by default.

`--allow-unvalidated-payloads` is available as an explicit diagnostic override.

## Runtime Options

```text
--rns-module <N>              Kaonic radio module to use for Reticulum
--local-interface <IFACE>     Local interface for ATAK multicast
--local-address <IPv4>        Local IPv4 address for ATAK multicast
--kaonic-ctrl-server <ADDR>   Kaonic control endpoint
--seed-key <KEY>              Gateway DB seed key for the plugin identity
--allow-unvalidated-payloads  Forward invalid payloads for diagnostics
```

The systemd service sets:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
ExecStart=/usr/bin/kaonic-atak-plugin --rns-module 0
```

If auto-detection is ambiguous on a device, edit the service or launch command
to include `--local-interface` or `--local-address`.

## Package

The installer package includes:

```text
kaonic-plugin.toml
kaonic-atak-plugin.service
kaonic-atak-plugin
kaonic-atak-plugin.sha256
```

Build and package example:

```bash
TARGET=armv7-unknown-linux-musleabihf
cross build --release -p kaonic-atak-plugin --target "$TARGET"
./scripts/package-plugin.sh "$TARGET"
```
