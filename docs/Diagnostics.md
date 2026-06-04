# Diagnostic Peer-Hash Tracking

## Purpose

The diagnostic control plane is a dormant-by-default mechanism for associating received ATAK Cursor-on-Target (CoT) events with the Reticulum peer that delivered them. It provides the transport-layer data needed by a future Kaonic diagnostics plugin without altering ATAK packets or exposing this information during normal operation.

The association answers a transport/debugging question:

```text
Reticulum peer hash -> ATAK UID/callsign -> event type and optional reported location
```

It does not redefine ATAK identity. The ATAK `uid` and optional callsign remain the identifiers for the reported track or chat sender; a peer hash identifies the Kaonic/Reticulum endpoint that delivered a packet to the local bridge.

## Behavior

Diagnostic tracking is disabled at startup. While disabled, validated ATAK traffic continues to be forwarded normally and peer-to-CoT diagnostic records are not retained.

When enabled, each receiving plugin stores a bounded, in-memory set of diagnostic records for valid remote CoT events. Records include:

- remote Reticulum peer hash;
- ATAK channel port;
- CoT UID, event type, and optional callsign;
- optional latitude and longitude when the event contains a point;
- local observation time.

Records are never inserted into the ATAK packet. The original validated CoT/GeoChat packet bytes are still forwarded unchanged to ATAK.

The diagnostic records are volatile. They are cleared when diagnostics are disabled, when the enable window expires, or when the plugin process exits. Persistent storage, richer telemetry, and visualization are reserved for a future diagnostics plugin.

## Network-wide control channel

Each plugin creates a Reticulum diagnostic control destination named:

```text
kaonic.atak.diag.control
```

Unauthenticated mesh control is disabled by default. When it is explicitly enabled for a trusted test mesh, nodes advertise and discover this destination separately from the ATAK data channels. A local enable or disable request creates a small versioned diagnostic command and relays it through discovered diagnostic control links. Each node applies each command identifier once and forwards a new command once so commands can propagate through a multi-node topology without looping indefinitely.

Propagation is best-effort. A node that joins after an enable or disable command has already traversed the mesh does not receive the current state until another local control command is issued. Operational use should replace this with an authenticated management protocol that also supports state synchronization.

Supported actions are:

- enable peer-hash diagnostic recording for a bounded duration;
- disable peer-hash diagnostic recording immediately.

Enable requests expire automatically. The default duration is 900 seconds (15 minutes); the maximum accepted duration is 86,400 seconds (24 hours).

Enable unauthenticated mesh control only on a trusted bench network:

```bash
kaonic-atak-plugin --enable-unauthenticated-diagnostics-mesh-control
```

or:

```bash
KAONIC_ATAK_ENABLE_UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL=true
```

## Local control socket

The plugin listens on the local loopback UDP control address by default:

```text
127.0.0.1:19001
```

The address can be changed with:

```bash
kaonic-atak-plugin --diagnostics-control-listen 127.0.0.1:19001
```

or:

```bash
KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN=127.0.0.1:19001
```

The loopback socket is the intended integration point for a later diagnostics plugin. Local command messages are UTF-8 text:

| Command | Meaning |
| --- | --- |
| `enable` | Enable local diagnostics for 900 seconds. If unauthenticated mesh control is explicitly enabled, also announce the command to discovered diagnostic peers. |
| `enable <seconds>` | Enable local diagnostics for a requested bounded duration. If unauthenticated mesh control is explicitly enabled, also announce the command to discovered diagnostic peers. |
| `disable` | Disable local diagnostics and clear retained records. If unauthenticated mesh control is explicitly enabled, also announce the command to discovered diagnostic peers. |
| `status` | Return local state and number of retained records. |
| `recent` | Return the most recent ten local diagnostic records. |
| `recent <1-20>` | Return up to the requested number of recent records. |

Example using netcat on a Kaonic node:

```bash
printf 'enable 900\n' | nc -u -w1 127.0.0.1 19001
printf 'status\n' | nc -u -w1 127.0.0.1 19001
printf 'recent 10\n' | nc -u -w1 127.0.0.1 19001
printf 'disable\n' | nc -u -w1 127.0.0.1 19001
```

A status reply has the form:

```text
OK enabled=true remaining_seconds=899 records=3
```

Each returned record has the form:

```text
RECORD unix_ms=<time> peer=<hash> port=<port> uid=<uid> callsign=<callsign> type=<type> lat=<lat-or-dash> lon=<lon-or-dash>
```

## Security boundary

This first control-plane implementation is designed for a trusted test mesh. It uses a loopback-only local control endpoint by default. When the explicit mesh-control override is enabled, it also uses Reticulum diagnostic links, but it does not yet add application-level signed authorization for network enable/disable commands.

Before operational use on a network where an untrusted Kaonic could participate, the diagnostics plugin should add an authorization layer, such as signed control commands accepted only from a designated management identity.

The service refuses non-loopback local-control bindings unless an explicit insecure override is supplied:

```bash
kaonic-atak-plugin \
  --diagnostics-control-listen 0.0.0.0:19001 \
  --allow-insecure-diagnostics-control-listen
```

or:

```bash
KAONIC_ATAK_ALLOW_INSECURE_DIAGNOSTICS_CONTROL_LISTEN=true
```

Do not use that override unless remote local-network control is explicitly intended and protected by a separate access-control design.

## Future diagnostics plugin integration

A future diagnostics plugin should use this control plane rather than changing ATAK/CoT data messages. Its first responsibilities can be:

1. Send `enable`, `disable`, and `status` requests to the local loopback interface.
2. Poll or subscribe to the diagnostic record output and display peer-to-ATAK associations.
3. Add authorization for management commands before broader deployment.
4. Optionally persist selected observations or export link-health metrics.

This preserves the division of responsibilities: ATAK supplies identity and position reports, the bridge transports packets, and a dedicated diagnostic component observes transport metadata only when explicitly enabled.
