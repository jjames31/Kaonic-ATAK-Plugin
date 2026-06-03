# Testing

## Purpose

Testing should establish three things before the plugin is used operationally:

1. the service starts on the intended Kaonic network interface;
2. valid ATAK traffic is carried through the radio path without modification;
3. the plugin does not transmit onto unrelated local networks or interact with attached hardware.

## Build-time checks

Run the available Rust checks before producing an installable ZIP:

```bash
cargo fmt --check
cargo test -p kaonic-atak-plugin
cargo check -p kaonic-atak-plugin
```

For a Kaonic deployment build, also create the ARMv7 release artifact:

```bash
cross build --release -p kaonic-atak-plugin --target armv7-unknown-linux-musleabihf
./scripts/package-plugin.sh armv7-unknown-linux-musleabihf
```

Record the exact commit and build result for each package installed on hardware.

## Installation and startup validation

After installing the plugin ZIP on a Kaonic, check the device addresses and service output:

```bash
ip -br addr
systemctl status kaonic-atak-plugin
journalctl -fu kaonic-atak-plugin
```

The service should select only the network interface intended for ATAK traffic. Without an explicit override, selection should succeed only when one suitable `192.168.10.x` address is present.

When the intended ATAK network uses a different address, configure it explicitly before proceeding:

```ini
Environment="KAONIC_ATAK_INTERFACE_IP=<kaonic-atak-facing-ip>"
```

## Local ATAK ingress test

Connect an ATAK-capable device to the selected Kaonic network and start ATAK location reporting. On the Kaonic, monitor the situational-awareness channel:

```bash
tcpdump -ni <atak-interface> udp port 6969
```

Review the plugin log for accepted CoT traffic and decoded location activity:

```bash
journalctl -fu kaonic-atak-plugin
```

Safe mode rejects arbitrary text or malformed UDP packets. Use real ATAK traffic or a valid CoT XML test event when validating packet ingress.

## Two-Kaonic end-to-end test

Required equipment:

- two Kaonics configured for radio/Reticulum connectivity;
- two ATAK-capable devices;
- the same tested plugin build installed on both Kaonics.

Test layout:

```text
ATAK A -- Kaonic A )) radio mesh (( Kaonic B -- ATAK B
```

Procedure:

1. Confirm that each plugin starts on its intended ATAK-facing network interface.
2. Confirm that ATAK A is producing CoT traffic on Kaonic A's local network.
3. Confirm that Kaonic B emits received CoT traffic only on its selected ATAK-facing interface.
4. Confirm that ATAK B displays ATAK A's location updates.
5. Repeat the test in the reverse direction.
6. Monitor unrelated active interfaces and confirm that the plugin does not emit ATAK multicast traffic onto them.

## Compatibility-mode test

Compatibility mode should be tested separately from normal operation. Enable it only when a test requires traffic the parser cannot validate:

```bash
KAONIC_ATAK_ALLOW_OPAQUE_FORWARDING=true
```

Return the service to safe mode when the compatibility test is complete.

## Remaining validation items

The following results should be documented after physical testing:

- Kaonic image/version used for installation;
- plugin commit and ZIP checksum;
- ATAK version and device type used for ingress testing;
- radio link configuration used for the two-device test;
- whether any duplicate or reflected packets were observed;
- whether a status or diagnostics interface is needed for decoded location data.
