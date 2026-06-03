# Kaonic ATAK Plugin

A custom Kaonic-hosted network adapter for transporting validated ATAK Cursor-on-Target (CoT) and GeoChat traffic across the Kaonic Reticulum radio mesh.

This repository is separate from Beechat's upstream `kaonic-gateway` repository. It is a Kaonic-side plugin/service, not an Android ATAK APK plugin.

## Current Status

The repository contains a custom plugin implementation baseline with:

- ATAK multicast-to-Reticulum and Reticulum-to-multicast bridging;
- CoT XML validation and decoded location tracking;
- interface-isolated multicast transmit behavior;
- fail-closed automatic ATAK interface selection;
- explicit opt-in compatibility forwarding for non-CoT payloads;
- ARMv7 cross-build and Kaonic package scripts.

Hardware validation still required:

- real ATAK phone packet ingress;
- two-Kaonic end-to-end radio delivery;
- plugin ZIP installation and runtime test on the current Kaonic image after the safety changes.

The upstream bridge was previously installed and smoke-tested on one Kaonic, but the current custom implementation must be rebuilt and revalidated before being considered deployable.

## Intended Architecture

```text
ATAK phone, tablet, or compatible network client
        |
        | UDP multicast on the selected ATAK-facing network
        v
Kaonic ATAK Plugin
        |
        | Reticulum over the Kaonic radio transport
        v
Remote Kaonic ATAK Plugin
        |
        | UDP multicast on its selected ATAK-facing network
        v
Remote ATAK phone, tablet, or compatible network client
```

## Supported Network Traffic

| Traffic type | Multicast group | Port | Purpose |
| --- | ---: | ---: | --- |
| SA / CoT | `239.2.3.1` | `6969` | Position, marker, and situational-awareness traffic |
| GeoChat / CoT | `224.10.10.1` | `17012` | Chat-related ATAK traffic |

The plugin forwards original packet bytes unchanged after validation. It parses location-bearing CoT events locally to track the latest known local and remote positions.

## Safety Model

Version 1 is a **network-only ATAK adapter**. It does not read from, configure, or control directly attached external hardware.

It does not implement or automatically activate:

- USB peripheral access;
- UART or serial input;
- external GPS/NMEA receivers;
- drone or MAVLink interfaces;
- GPIO, SPI, or I2C devices;
- cameras, sensors, or accessory power-control behavior.

A device connected to the Kaonic is relevant to this plugin only if it intentionally sends supported ATAK-compatible network traffic on the selected ATAK-facing network interface.

### Safe forwarding mode

Safe mode is the default. In this mode, only valid CoT XML event packets on the supported ATAK multicast channels are sent through Reticulum or emitted back onto the local ATAK network. Malformed XML and unrelated arbitrary UDP payloads are dropped.

### Compatibility mode

Opaque forwarding is available only through explicit opt-in configuration:

```bash
--allow-unvalidated-payloads
```

or:

```bash
KAONIC_ATAK_ALLOW_OPAQUE_FORWARDING=true
```

Compatibility mode may transport payloads that the plugin cannot verify as ATAK CoT. Use it only during controlled compatibility testing.

## ATAK-Facing Interface Selection

The plugin emits local multicast only on one selected ATAK-facing IPv4 interface. It does not retransmit onto every non-loopback interface.

### Recommended explicit configuration

Select the ATAK-facing address with either:

```bash
--local-address <IPv4>
```

or:

```bash
KAONIC_ATAK_INTERFACE_IP=<IPv4>
```

An interface name can also be constrained with:

```bash
--local-interface <name>
```

or:

```bash
KAONIC_ATAK_INTERFACE=<name>
```

CLI options override environment variables.

### Automatic selection

Without explicit configuration, the plugin will use the network interface only when exactly one non-loopback address is present on the expected Kaonic ATAK LAN subnet:

```text
192.168.10.0/24
```

When no such address exists, or selection is ambiguous, startup fails closed rather than transmitting onto an unrelated network interface. This behavior is deliberate when a Kaonic is attached to other hardware or networks.

## CoT Location Interpretation

For valid CoT packets with a usable `<point>` element, the plugin extracts and tracks:

- UID;
- event type;
- `how` method field when present;
- callsign from `<detail><contact ...>` when present;
- latitude and longitude;
- optional HAE, CE, and LE values;
- CoT time, start, and stale metadata when present.

Valid CoT events without a location point are still forwardable; they simply do not create a location record.

The location store:

- maintains local-to-mesh and mesh-to-local records separately;
- is bounded to avoid unlimited UID growth;
- removes records after a retention period.

## Plugin Files

```text
Kaonic-ATAK-Plugin/
├── README.md
├── Cargo.toml
├── Cross.toml
├── scripts/
│   └── package-plugin.sh
├── kaonic-atak-plugin/
│   ├── Cargo.toml
│   ├── kaonic-plugin.toml
│   ├── kaonic-atak-plugin.service
│   └── src/
│       ├── main.rs
│       ├── cot.rs
│       ├── interface.rs
│       └── multicast.rs
├── kaonic-gateway/
├── kaonic-reticulum/
└── kaonic-vpn/
```

## Build and Package

The target Kaonic tested previously reported `armv7l`. The intended cross-build target is:

```bash
armv7-unknown-linux-musleabihf
```

Build and package:

```bash
cross build --release -p kaonic-atak-plugin --target armv7-unknown-linux-musleabihf
./scripts/package-plugin.sh armv7-unknown-linux-musleabihf
```

The resulting ZIP should contain:

```text
kaonic-plugin.toml
kaonic-atak-plugin.service
kaonic-atak-plugin
kaonic-atak-plugin.sha256
```

## Runtime Prerequisites

Known device assumptions from prior testing of the upstream reference bridge:

```text
Kaonic control endpoint: 192.168.10.1:9090
Gateway database path:  /kaonic-gateway.db
Device architecture:     armv7l
```

The service file configures:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
```

For deployments that do not expose exactly one intended ATAK-facing `192.168.10.x` address, set `KAONIC_ATAK_INTERFACE_IP` before starting the service.

## Local Validation Procedure

After building, installing, and starting the plugin on a Kaonic:

```bash
ip -br addr
journalctl -fu kaonic-atak-plugin
```

Confirm that startup logs identify only the intended ATAK-facing interface. Then, from an ATAK-compatible device connected on that network, verify traffic on the CoT channel:

```bash
tcpdump -ni <interface> udp port 6969
```

In safe mode, arbitrary test text is expected to be rejected. Use a valid CoT XML packet for packet-ingress validation.

## End-to-End Test Procedure

Use two Kaonics and two ATAK-compatible devices:

```text
ATAK A -- Kaonic A )) Reticulum/radio (( Kaonic B -- ATAK B
```

Verify:

1. each plugin selects only its intended ATAK-facing interface;
2. valid local CoT data is received and forwarded into Reticulum;
3. the opposite Kaonic receives and rebroadcasts that data locally;
4. ATAK B displays ATAK A location updates;
5. reverse-direction operation also works;
6. unrelated network interfaces do not receive plugin-emitted ATAK multicast.

## Known Limitations

- Real ATAK and two-radio operation have not yet been validated for the custom safety-hardened implementation.
- Direct GPS accessory, NMEA, MAVLink, serial, USB peripheral, and arbitrary hardware support are intentionally not implemented.
- Multicast loopback is disabled on plugin-originated transmit sockets; additional duplicate/echo suppression should be considered if testing identifies topology-dependent rebroadcast loops.
- No web status endpoint is currently exposed for decoded location state or counters.

## Upstream Reference

Beechat's `kaonic-atak-bridge` implementation in `kaonic-gateway` remains a read-only reference for multicast/Reticulum transport patterns and packaging conventions. All custom behavior in this repository should be implemented and maintained here.
