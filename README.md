# Kaonic ATAK Plugin

A Kaonic-side service for carrying ATAK Cursor-on-Target (CoT) and GeoChat traffic over the Kaonic Reticulum radio mesh.

This is a network bridge for a Kaonic device. It is not an Android ATAK plugin, and it does not read GPS data directly from attached hardware. A phone, tablet, or computer running ATAK-compatible software provides the CoT traffic on the local network.

## Status

The plugin has an implementation baseline for validated CoT forwarding, location parsing, interface-isolated multicast output, and an opt-in diagnostic peer-hash control plane intended for future diagnostics-plugin integration. It still requires build verification and testing on physical Kaonic and ATAK hardware before deployment.

## Supported traffic

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |

## Key behavior

- Forwards validated ATAK CoT traffic between the local ATAK network and Reticulum.
- Reads location-bearing CoT messages for local position tracking without modifying the transmitted packet bytes.
- Supports a dormant-by-default diagnostic control channel that can temporarily record `Reticulum peer hash -> CoT UID/callsign/event` associations across participating plugin nodes.
- Exposes a loopback-only local diagnostics command interface for later integration by a dedicated diagnostics plugin.
- Sends multicast traffic only on the selected ATAK-facing interface.
- Fails closed when it cannot safely identify that interface.
- Does not access USB, UART, GPS receivers, drones, cameras, or other attached peripherals.

## Documentation

- ### [Wiki](https://github.com/jjames31/Kaonic-ATAK-Plugin/wiki)
- [Documentation home](docs/Home.md)
- [Design and safety](docs/Design-and-Safety.md)
- [Configuration](docs/Configuration.md)
- [Diagnostic peer-hash tracking](docs/Diagnostics.md)
- [Build and install](docs/Build-and-Install.md)
- [Testing](docs/Testing.md)

## Quick build

```bash
cross build --release -p kaonic-atak-plugin --target armv7-unknown-linux-musleabihf
./scripts/package-plugin.sh armv7-unknown-linux-musleabihf
```

The resulting package is written to:

```text
deploy/kaonic-atak-plugin/kaonic-atak-plugin.zip
```

## Reference implementation

Beechat's `kaonic-atak-bridge` in `kaonic-gateway` is used as an upstream reference for Kaonic transport and packaging patterns. This repository contains the custom implementation for this project.




# - [**Download**](https://github.com/jjames31/Kaonic-ATAK-Plugin/tree/main/Builds)
