# Testing

## Purpose

Testing should establish four things before the plugin is used operationally:

1. the service starts on the intended Kaonic network interface;
2. valid ATAK traffic is carried through the radio path without modification;
3. the plugin does not transmit onto unrelated local networks or interact with attached hardware;
4. optional diagnostic peer-hash tracking stays off by default and propagates enable/disable commands only when unauthenticated mesh control is explicitly enabled for deliberate trusted-mesh testing.

## Build-time checks

Run the available Rust checks before producing an installable ZIP:

```bash
cargo fmt --check
cargo test -p kaonic-atak-plugin
cargo check -p kaonic-atak-plugin
```

The unit tests include diagnostic command parsing, duplicate-command suppression, and confirmation that peer-hash records are captured only while diagnostics are enabled.

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

The diagnostic local-control socket should bind to loopback by default and report disabled state:

```bash
printf 'status\n' | nc -u -w1 127.0.0.1 19001
```

Expected form:

```text
OK enabled=false remaining_seconds=0 records=0
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

## Diagnostic control-plane test

Run this test only after ordinary ATAK delivery works. Diagnostics must remain disabled during normal forwarding validation.

1. On both Kaonics, verify the initial state:

   ```bash
   printf 'status\n' | nc -u -w1 127.0.0.1 19001
   ```

2. Enable unauthenticated diagnostics mesh control on both test Kaonics. This is for trusted bench testing only:

   ```ini
   Environment="KAONIC_ATAK_ENABLE_UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL=true"
   ```

3. On Kaonic A, request a short network-wide enable window:

   ```bash
   printf 'enable 120\n' | nc -u -w1 127.0.0.1 19001
   ```

4. On Kaonic B, confirm that the control command propagated:

   ```bash
   printf 'status\n' | nc -u -w1 127.0.0.1 19001
   ```

   `enabled=true` should be reported with a decreasing `remaining_seconds` value.

5. Generate ATAK location or chat CoT traffic through the radio path. On the receiving node, query recent diagnostics:

   ```bash
   printf 'recent 10\n' | nc -u -w1 127.0.0.1 19001
   ```

   Each received record should include the Reticulum peer hash and the CoT UID/type, with latitude and longitude only when supplied by the CoT event.

6. Disable tracking from either node and confirm that the disabled state propagates:

   ```bash
   printf 'disable\n' | nc -u -w1 127.0.0.1 19001
   ```

7. Generate additional CoT traffic and verify that the retained-record count remains zero after disable.

8. Capture ATAK traffic before, during, and after diagnostics and confirm that the forwarded ATAK packet bytes remain unchanged.

The current mesh control plane is for trusted testing only; it is not an authorization validation test for operational deployment.

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
- whether diagnostics enable/disable propagated to each participating Kaonic;
- whether future diagnostic telemetry needs persistence, visualization, or signed control authorization.
