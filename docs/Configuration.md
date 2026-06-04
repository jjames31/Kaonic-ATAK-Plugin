# Configuration

## Overview

The plugin is designed to connect one local ATAK-facing network to the Kaonic radio mesh. For safety, it uses one local IPv4 interface for multicast input and output rather than broadcasting onto every interface available on the device.

## Selecting the ATAK-facing interface

The most predictable configuration is to identify the IPv4 address on the network used by the attached ATAK device and set it explicitly:

```bash
KAONIC_ATAK_INTERFACE_IP=192.168.10.1
```

The same setting can be supplied as a command-line option:

```bash
kaonic-atak-plugin --local-address 192.168.10.1
```

An interface name may also be specified:

```bash
KAONIC_ATAK_INTERFACE=wlan0
```

or:

```bash
kaonic-atak-plugin --local-interface wlan0
```

Command-line arguments take precedence over environment variables.

## Automatic selection

When no interface is configured, the plugin looks for an ATAK-facing address on `192.168.10.0/24`.

- If exactly one matching address is found, it is used.
- If no matching address is found, startup fails without sending local ATAK multicast traffic.
- If more than one matching address is found, startup fails and an explicit selection is required.

This fail-closed behavior prevents the plugin from sending remote ATAK traffic onto an unrelated network when other hardware is attached to the Kaonic.

## Forwarding modes

### Safe mode

Safe mode is the normal operating mode. The plugin forwards valid CoT XML events received on the supported ATAK channels. Payloads that cannot be validated as CoT are rejected.

No configuration is needed to enable safe mode.

### Compatibility mode

Compatibility mode forwards payloads even when the plugin cannot validate them as CoT. It is intended for controlled troubleshooting or compatibility testing only.

Enable it with one of the following:

```bash
KAONIC_ATAK_ALLOW_OPAQUE_FORWARDING=true
```

```bash
kaonic-atak-plugin --allow-unvalidated-payloads
```

Do not enable compatibility mode on a network where arbitrary devices may transmit to the ATAK multicast channels unless that behavior is intentional.

## Diagnostic peer-hash tracking

Diagnostic tracking is independent from ATAK forwarding. It is disabled by default and, when enabled, temporarily records the Reticulum peer hash associated with valid remote CoT events. ATAK messages are not altered.

Each Kaonic plugin exposes a local Unix datagram control socket by default. The default local control path is:

```text
/run/kaonic-atak-plugin/diagnostics.sock
```

Override it with a command-line option:

```bash
kaonic-atak-plugin --diagnostics-unix-socket /run/kaonic-atak-plugin/diagnostics.sock
```

or an environment variable:

```bash
KAONIC_ATAK_DIAGNOSTICS_UNIX_SOCKET=/run/kaonic-atak-plugin/diagnostics.sock
```

The packaged service creates `/run/kaonic-atak-plugin` through systemd `RuntimeDirectory` and the plugin sets the socket mode to `0600`. A local diagnostics client must bind its own Unix datagram reply socket before sending a command.

To run with no local diagnostics control endpoint at all, use:

```bash
kaonic-atak-plugin --disable-local-diagnostics-control
```

or:

```bash
KAONIC_ATAK_DISABLE_LOCAL_DIAGNOSTICS_CONTROL=true
```

UDP local diagnostics control is retained only as an explicit compatibility/test option:

```bash
KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN=127.0.0.1:19001
```

The service refuses non-loopback UDP diagnostics-control bindings by default. For a controlled test that intentionally exposes the UDP control socket, set an explicit insecure override:

```bash
KAONIC_ATAK_ALLOW_INSECURE_DIAGNOSTICS_CONTROL_LISTEN=true
```

Do not use this override unless local-network control is explicitly required and protected separately.

Diagnostics control startup is optional by default. If the Unix socket or explicit UDP socket cannot be created, ATAK forwarding continues and the service logs that diagnostics control is unavailable. To make diagnostics control startup fatal for a validation run, set:

```bash
KAONIC_ATAK_REQUIRE_DIAGNOSTICS_CONTROL=true
```

### Local commands

A local CLI or a future diagnostics plugin can send the following UTF-8 text commands to the Unix diagnostics socket:

| Command | Behavior |
| --- | --- |
| `enable` | Enable local diagnostics for 900 seconds. |
| `enable <seconds>` | Enable for 1 to 86,400 seconds. |
| `disable` | Disable local diagnostics and clear retained diagnostic records. |
| `status` | Return local enable state and retained-record count. |
| `recent [1-20]` | Return bounded recent peer-to-CoT records stored locally. |

Example using Python on a Kaonic:

```bash
python3 - <<'PY'
import os, socket, tempfile
server = "/run/kaonic-atak-plugin/diagnostics.sock"
client = tempfile.mktemp(prefix="kaonic-atak-diag-", suffix=".sock")
sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
sock.bind(client)
sock.sendto(b"status\n", server)
print(sock.recv(4096).decode(), end="")
sock.close()
os.unlink(client)
PY
```

Unauthenticated network-wide enable/disable propagation is disabled by default. For a trusted bench mesh, enable it explicitly:

```bash
KAONIC_ATAK_ENABLE_UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL=true
```

This control channel is currently intended for trusted development/test meshes only. Operational mesh diagnostics remain unsupported until signed management authorization and state synchronization are implemented. See [Diagnostic Peer-Hash Tracking](Diagnostics.md) for the data boundary and future plugin integration plan.

## Service configuration

The service file sets the gateway database location used on the Kaonic image:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
```

For deployments where automatic selection is not appropriate, add an interface address override to the installed service configuration before starting the plugin:

```ini
Environment="KAONIC_ATAK_INTERFACE_IP=192.168.10.1"
```

To enable the compatibility loopback UDP diagnostics-control port, add:

```ini
Environment="KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN=127.0.0.1:19001"
```

To select a non-default Unix diagnostics socket path, add:

```ini
Environment="KAONIC_ATAK_DIAGNOSTICS_UNIX_SOCKET=/run/kaonic-atak-plugin/diagnostics.sock"
```

To enable unauthenticated diagnostics propagation for a trusted bench mesh only, add:

```ini
Environment="KAONIC_ATAK_ENABLE_UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL=true"
```

Then reload systemd and restart the service:

```bash
systemctl daemon-reload
systemctl restart kaonic-atak-plugin
```

## Relevant multicast channels

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |
