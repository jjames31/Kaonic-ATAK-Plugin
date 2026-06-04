# Kaonic ATAK Plugin Documentation

The Kaonic ATAK Plugin is a Kaonic-side service that carries ATAK Cursor-on-Target (CoT) traffic over the Kaonic radio mesh. A phone, tablet, or computer running ATAK-compatible software connects to the Kaonic network and continues to handle its own GPS and map display. The plugin transports and interprets the network traffic it receives; it does not read location directly from attached hardware.

## Documentation

- [Design and safety](Design-and-Safety.md) — architecture, supported traffic, location parsing, diagnostic data boundaries, and hardware boundaries.
- [Configuration](Configuration.md) — interface selection, forwarding modes, diagnostic-control commands, and service configuration.
- [Diagnostic peer-hash tracking](Diagnostics.md) — local toggle behavior, explicit trusted-mesh propagation, and the integration point for a future diagnostics plugin.
- [Build and install](Build-and-Install.md) — building the ARMv7 binary and packaging a Kaonic plugin ZIP.
- [Testing](Testing.md) — local validation and two-Kaonic end-to-end testing.
- [Change log](Change-Log.md) — packaged release notes and validation status.

## Project status

The repository contains an implementation baseline for validated ATAK traffic bridging and CoT location tracking. The v0.1.1 package has local build and package verification for the Kaonic ARMv7 target, and the safety-related code fails closed when an ATAK-facing network interface cannot be selected unambiguously.

The custom plugin still needs hardware validation before operational deployment:

- installation and startup testing on the current Kaonic image;
- real ATAK phone ingress testing;
- two-Kaonic radio delivery testing;
- Unix-socket diagnostics-query testing and trusted-mesh enable/disable propagation when explicitly enabled.

## Supported ATAK channels

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |

Valid CoT packets are forwarded byte-for-byte after validation. Location-bearing messages are also decoded locally so that the plugin can track the latest known local and remote positions. While diagnostics are explicitly enabled, valid remote CoT events can additionally be associated with the Reticulum peer hash that delivered them, without modifying the ATAK packets.
