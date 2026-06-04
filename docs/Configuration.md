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

Each Kaonic plugin exposes a local loopback UDP control interface and carries enable/disable commands over a separate Reticulum diagnostic control destination. The default local control address is:

```text
127.0.0.1:19001
```

Override it with a command-line option:

```bash
kaonic-atak-plugin --diagnostics-control-listen 127.0.0.1:19001
```

or an environment variable:

```bash
KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN=127.0.0.1:19001
```

Do not bind this endpoint to a non-loopback interface unless local-network control is explicitly required and secured.

### Local commands

A local CLI or a future diagnostics plugin can send the following UDP text commands to `127.0.0.1:19001`:

| Command | Behavior |
| --- | --- |
| `enable` | Enable diagnostics across participating plugin nodes for 900 seconds. |
| `enable <seconds>` | Enable for 1 to 86,400 seconds. |
| `disable` | Disable diagnostics across participating plugin nodes. |
| `status` | Return local enable state and retained-record count. |
| `recent [1-20]` | Return bounded recent peer-to-CoT records stored locally. |

Example:

```bash
printf 'enable 900\n' | nc -u -w1 127.0.0.1 19001
printf 'recent 10\n' | nc -u -w1 127.0.0.1 19001
printf 'disable\n' | nc -u -w1 127.0.0.1 19001
```

This control channel is currently intended for trusted development/test meshes. It should be extended with signed management authorization before operational deployment with untrusted nodes. See [Diagnostic Peer-Hash Tracking](Diagnostics.md) for the data boundary and future plugin integration plan.

## Service configuration

The service file sets the gateway database location used on the Kaonic image:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
```

For deployments where automatic selection is not appropriate, add an interface address override to the installed service configuration before starting the plugin:

```ini
Environment="KAONIC_ATAK_INTERFACE_IP=192.168.10.1"
```

To select a non-default loopback diagnostics-control port, add:

```ini
Environment="KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN=127.0.0.1:19001"
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
