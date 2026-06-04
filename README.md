# Kaonic ATAK Plugin

A Kaonic-side service for carrying ATAK Cursor-on-Target (CoT) and GeoChat traffic over the Kaonic Reticulum radio mesh.

This is a network bridge that runs on a Kaonic device. It is not an Android ATAK plugin, and it does not read GPS data directly from attached hardware. A phone, tablet, or computer running ATAK-compatible software provides the CoT traffic on the local network; this service carries that traffic across the radio mesh.

## Why this project exists

Forwarding ATAK traffic over a radio mesh is the starting point. When working with multiple Kaonics in a real test setup, it is also useful to know whether the traffic is valid, where received tracks came from, and whether the bridge is sending data back onto the correct local network interface.

This implementation follows the Kaonic ATAK bridge pattern for CoT and GeoChat transport, while adding diagnostic and safety features around that path. Normal ATAK CoT packets are forwarded without rewriting their contents, so ATAK remains responsible for positions, callsigns, chat, markers, and other supported CoT behavior.

Because it performs the same ATAK-to-Reticulum bridge role, this plugin is intended to replace the default `kaonic-atak-bridge.service`, not run alongside it. When this plugin service is started, its systemd unit conflicts with and disables the default bridge to avoid duplicate forwarded packets and competing multicast bridges.

## Status

The plugin has an implementation baseline for validated CoT forwarding, location parsing, interface-isolated multicast output, default bridge replacement, and an opt-in diagnostic peer-hash control plane intended for future diagnostics-plugin integration. It still requires build verification and testing on physical Kaonic and ATAK hardware before deployment.

## Supported traffic

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |

## Core bridge behavior

- Carries ATAK CoT and GeoChat packets between the local ATAK network and Reticulum.
- Establishes Reticulum links with compatible bridge peers advertising the same ATAK channel.
- Preserves accepted ATAK packet bytes while they cross the mesh.
- Uses the Kaonic radio configuration and selected radio module for transport.

## What this implementation adds

| Addition | Why it matters |
| --- | --- |
| Replaces the default ATAK bridge service | The custom bridge conflicts with and disables `kaonic-atak-bridge.service` when started, preventing two services from handling the same ATAK multicast channels at the same time. |
| CoT validation before forwarding | Malformed or unrelated local UDP data is dropped by default instead of consuming radio bandwidth. An explicit compatibility option is available when opaque forwarding is required for testing. |
| Location-aware parsing | The service can read UID, callsign, event type, position, accuracy, altitude, and stale/time metadata from valid CoT events without changing what ATAK receives. This makes later diagnostics possible without inventing a separate position protocol. |
| Bounded local and remote location state | Recent location observations can be inspected during development without allowing the service's memory usage to grow indefinitely. |
| Optional peer-hash diagnostics | During controlled testing, the plugin can temporarily record which Reticulum peer delivered a CoT UID, callsign, or event. This is useful when several Kaonics are active and a received ATAK contact needs to be traced back to its mesh endpoint. |
| Loopback-only diagnostic control interface | A future diagnostics UI can query or enable tracking locally without exposing the control endpoint on the ATAK-facing network by default. |
| Interface-isolated multicast output | Received mesh traffic is sent only to the chosen ATAK-facing interface, rather than being rebroadcast on every local interface attached to the Kaonic. |
| Fail-closed interface selection | If the plugin cannot safely determine which interface belongs to the ATAK connection, it refuses to start instead of potentially transmitting data onto the wrong network. |
| Network-only safety boundary | The service does not probe USB, UART, GPS receivers, cameras, drones, or other attached hardware. Connecting a peripheral does not automatically make it a data source for the bridge. |

## Documentation

- [Wiki](https://github.com/jjames31/Kaonic-ATAK-Plugin/wiki)
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

Beechat's `kaonic-atak-bridge` in `kaonic-gateway` is used as an upstream reference for Kaonic transport and packaging patterns. This repository keeps that transport role, then adds the validation, interface isolation, diagnostic hooks, and service replacement behavior needed for this project's testing and future tools.

## Download

Packaged builds, when available, are stored in the [Builds](https://github.com/jjames31/Kaonic-ATAK-Plugin/tree/main/Builds) directory.
