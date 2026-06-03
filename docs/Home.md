# Kaonic ATAK Plugin Documentation

The Kaonic ATAK Plugin is a Kaonic-side service that carries ATAK Cursor-on-Target (CoT) traffic over the Kaonic radio mesh. A phone, tablet, or computer running ATAK-compatible software connects to the Kaonic network and continues to handle its own GPS and map display. The plugin transports and interprets the network traffic it receives; it does not read location directly from attached hardware.

## Documentation

- [Design and safety](Design-and-Safety.md) — architecture, supported traffic, location parsing, and hardware boundaries.
- [Configuration](Configuration.md) — interface selection, safe mode, compatibility mode, and service configuration.
- [Build and install](Build-and-Install.md) — building the ARMv7 binary and packaging a Kaonic plugin ZIP.
- [Testing](Testing.md) — local validation and two-Kaonic end-to-end testing.

## Project status

The repository contains an implementation baseline for validated ATAK traffic bridging and CoT location tracking. The safety-related code has been updated to fail closed when an ATAK-facing network interface cannot be selected unambiguously.

The custom plugin still needs hardware validation before deployment:

- build and package verification for the Kaonic ARMv7 target;
- installation and startup testing on the current Kaonic image;
- real ATAK phone ingress testing;
- two-Kaonic radio delivery testing.

## Supported ATAK channels

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |

Valid CoT packets are forwarded byte-for-byte after validation. Location-bearing messages are also decoded locally so that the plugin can track the latest known local and remote positions.
