# Design and Safety

## Purpose

The Kaonic ATAK Plugin connects ATAK-compatible network traffic to the Kaonic radio mesh. It runs on the Kaonic device and is intended for deployments where an ATAK phone, tablet, or computer is connected to the Kaonic local network.

The plugin is not an ATAK Android application. ATAK obtains the device location and produces Cursor-on-Target (CoT) messages; the plugin carries those messages over the mesh and reads location fields for local tracking.

## Data flow

```text
ATAK device A
    │  CoT / GeoChat multicast
    ▼
Kaonic A running kaonic-atak-plugin
    │  Reticulum over radio
    ▼
Kaonic B running kaonic-atak-plugin
    │  CoT / GeoChat multicast
    ▼
ATAK device B
```

## Supported channels

| Traffic | Multicast group | Port | Use |
| --- | ---: | ---: | --- |
| Situational awareness / CoT | `239.2.3.1` | `6969` | Positions, markers, and related situational-awareness events |
| GeoChat / CoT | `224.10.10.1` | `17012` | ATAK chat traffic |

After a packet is validated, the original bytes are transported without modification. This keeps the bridge compatible with ATAK while allowing the plugin to observe supported CoT data.

## Location interpretation

When a valid CoT event includes a usable `<point>` element, the plugin reads the following fields when present:

- UID and event type;
- `how` field;
- callsign from contact details;
- latitude and longitude;
- HAE, CE, and LE values;
- time, start, and stale metadata.

Valid CoT packets that do not include a location remain eligible for forwarding; they simply do not create a location record.

The plugin maintains separate recent-location records for local packets sent into the mesh and remote packets received from the mesh. The state is bounded and old entries are pruned so the service does not grow memory usage indefinitely.

## Safety boundaries

This version is deliberately network-only. It does not probe, configure, or communicate with attached peripherals such as:

- USB devices;
- serial or UART devices;
- external GPS receivers;
- drones or MAVLink equipment;
- GPIO, SPI, or I2C accessories;
- cameras or sensors.

Connecting unrelated hardware to a Kaonic does not make it a data source for this plugin. A local device matters only when it sends supported ATAK network traffic through the configured ATAK-facing interface.

## Network isolation

The plugin sends ATAK multicast traffic only on one selected local interface. It does not rebroadcast remote packets over every network interface on the Kaonic.

When no interface is explicitly configured, automatic selection succeeds only when exactly one suitable address exists on the expected `192.168.10.0/24` local ATAK network. If there is no match or more than one match, the service refuses to start rather than choosing an unrelated network.

## Forwarding policy

Safe mode is the default. Invalid XML and unrelated payloads are dropped instead of being relayed across the radio path.

An explicit compatibility mode is available for controlled tests that require opaque payload forwarding. Because it relaxes validation, it should not be used by default on operational networks.
