# SSH Status and Troubleshooting

## Purpose

The Kaonic ATAK Plugin is intended to be observable from the Kaonic command line during setup and field testing. When a phone appears connected but ATAK contacts or chat messages are not making it across the mesh, the service log should show where the data path stops:

```text
ATAK device -> local multicast -> kaonic-atak-plugin -> Reticulum/radio -> remote plugin -> local multicast -> ATAK device
```

The packaged systemd service enables verbose journal output for the plugin and the related Kaonic/Reticulum transport components. This is especially useful while testing two or more Kaonics, confirming that the default ATAK bridge is disabled, and diagnosing radio or local-network problems.

## Logging enabled by the service

The installed service includes:

```ini
Environment="RUST_LOG=info,kaonic_atak_plugin=debug,kaonic_gateway=info,kaonic_reticulum=info,kaonic_ctrl=info,reticulum=info"
SyslogIdentifier=kaonic-atak-plugin
StandardOutput=journal
StandardError=journal
```

At this level, normal plugin forwarding decisions and connection activity are visible without setting every dependency to full debug output. The current source logs location-bearing CoT messages at debug level. Those messages may include ATAK UID, callsign, latitude, and longitude, so exported logs should be treated as operationally sensitive.

## Quick live view over SSH

After SSHing into a Kaonic, first check whether the replacement service is active and the upstream bridge is not running beside it:

```bash
systemctl --no-pager --full status kaonic-atak-plugin.service
systemctl is-active kaonic-atak-bridge.service
systemctl is-enabled kaonic-atak-bridge.service
```

Expected state after the custom plugin is installed and started:

```text
kaonic-atak-plugin.service: active
kaonic-atak-bridge.service: inactive
kaonic-atak-bridge.service: disabled
```

Follow the custom plugin log live while generating ATAK traffic:

```bash
journalctl -fu kaonic-atak-plugin.service -o short-iso
```

For a simpler output stream without journal metadata:

```bash
journalctl -fu kaonic-atak-plugin.service -o cat
```

Show all plugin output from the current boot:

```bash
journalctl -u kaonic-atak-plugin.service -b --no-pager -o short-iso
```

## What a healthy startup should show

A normal startup should produce messages indicating that:

1. The plugin selected exactly one ATAK-facing interface and IPv4 address.
2. Safe forwarding mode is enabled, unless compatibility mode was intentionally configured.
3. The plugin joined the CoT and GeoChat channels.
4. The local diagnostics control endpoint started on loopback.
5. A peer bridge is discovered and a Reticulum link becomes active once a second Kaonic running this plugin is reachable.

Typical plugin messages include patterns such as:

```text
using local ATAK interface <interface> (<address>)
safe forwarding mode enabled: invalid non-CoT payloads will be dropped
atak-plugin:cot:6969 joined via <interface> (<address>) dest=<hash>
atak-plugin:geochat:17012 joined via <interface> (<address>) dest=<hash>
atak-plugin:diagnostics listening locally on 127.0.0.1:19001 dest=<hash> (disabled by default)
atak-plugin:cot:6969 auto-link -> <peer-hash>
atak-plugin:cot:6969 link activated <peer-hash>
```

The peer discovery and link activation messages only occur once another compatible bridge is visible on the radio mesh.

## Watching an ATAK test

For an initial two-Kaonic test, open the live journal on both radios:

```bash
journalctl -fu kaonic-atak-plugin.service -o short-iso
```

Then send a location update, marker, or chat message from ATAK on one side. On the transmitting Kaonic, a valid packet should result in local UDP-to-Reticulum activity. For a location-bearing packet, debug output should also identify its CoT UID, callsign when present, and coordinate fields. On the receiving Kaonic, the log should show the remote CoT event being accepted before it is published back to local ATAK multicast.

Useful activity patterns include:

```text
atak-plugin:<channel>:<port> udp -> rns <bytes>B from <local-address>
atak-plugin:<channel>:<port> CoT source=LocalUdp uid=<uid> ...
atak-plugin:<channel>:<port> CoT source=RemoteReticulum uid=<uid> ...
atak-plugin:<channel>:<port> accepted valid non-location CoT uid=<uid> ...
```

A position update has a location record. A chat or other valid non-location CoT event may appear as an accepted non-location event instead.

## Temporarily correlating an ATAK contact with a peer Kaonic

The normal bridge forwards ATAK traffic without replacing ATAK identities. During a controlled multi-radio troubleshooting session, the optional diagnostic control plane can temporarily retain the relationship between a remote Reticulum peer and the CoT UID/callsign received from it.

On a Kaonic where `nc` is available, enable tracking for 15 minutes and inspect the most recent records:

```bash
printf 'enable 900\n' | nc -u -w1 127.0.0.1 19001
printf 'recent 10\n' | nc -u -w1 127.0.0.1 19001
```

Disable the tracking session when finished:

```bash
printf 'disable\n' | nc -u -w1 127.0.0.1 19001
```

This feature is for trusted development and test meshes. It intentionally exposes contact-to-peer correlation while enabled and should not be treated as an operational authorization mechanism.

## Common error messages and next checks

| Log message or symptom | Meaning | Next check |
| --- | --- | --- |
| `local ATAK interface selection failed` | The plugin could not safely determine the ATAK-facing local network. | Configure `KAONIC_ATAK_INTERFACE_IP` or pass `--local-address` for the actual ATAK-facing address. |
| `kaonic-ctrl connect error` | The plugin could not reach the Kaonic controller service. | Verify that `kaonic-commd.service` or the device controller backend is running and that the configured controller address is correct. |
| `radio interface attach error` | Reticulum could not be attached to the selected radio module. | Verify the selected `--rns-module` and inspect radio/controller logs. |
| `keepalive ping failed` | The running plugin lost or cannot maintain its controller connection. | Inspect controller health and restart the service after confirming the controller is available. |
| `udp receive error` or `udp send error` | The local ATAK multicast socket could not read or publish traffic. | Verify the selected local interface is still up and has the expected IP address. |
| `dropping invalid ATAK payload` | A packet on an ATAK multicast channel was not accepted as valid CoT XML. | Confirm the sender is producing CoT. Use compatibility mode only for deliberate testing of non-CoT payloads. |
| No `auto-link` or `link activated` entry | No compatible remote plugin bridge has been discovered over Reticulum. | Verify the other Kaonic is running this plugin and that both radios are configured for a compatible link. |
| Remote ATAK receives duplicated contacts/messages | More than one bridge may be active on a device. | Confirm `kaonic-atak-bridge.service` is inactive/disabled on every Kaonic using the custom plugin. |

## Changing verbosity

The packaged unit uses a useful troubleshooting default. For a quieter deployed service, override the environment variable with fewer debug targets. For deeper development testing, expand the debug targets temporarily. For example, to enable debug output from the plugin and Reticulum components through a systemd override:

```bash
systemctl edit kaonic-atak-plugin.service
```

Add:

```ini
[Service]
Environment="RUST_LOG=info,kaonic_atak_plugin=debug,kaonic_gateway=debug,kaonic_reticulum=debug,kaonic_ctrl=debug,reticulum=debug"
```

Then apply the change:

```bash
systemctl daemon-reload
systemctl restart kaonic-atak-plugin.service
journalctl -fu kaonic-atak-plugin.service -o short-iso
```

Remove the override after the test if the additional log volume is no longer needed:

```bash
systemctl revert kaonic-atak-plugin.service
systemctl restart kaonic-atak-plugin.service
```

## Collecting a troubleshooting log

Capture logs from the current boot for later review:

```bash
journalctl -u kaonic-atak-plugin.service -b -o short-iso --no-pager > /tmp/kaonic-atak-plugin-current-boot.log
```

Before sharing a log, review it for callsigns, ATAK UIDs, positions, peer hashes, and other information that should not leave the test group.
