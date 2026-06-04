# Change Log

This log summarizes the packaged Kaonic ATAK Plugin builds. It focuses on operator-visible behavior, packaging status, and validation notes rather than every internal commit.

## v0.1.2

Current package:

```text
Builds/KATAK-v0.1.2.zip
```

This build continues from v0.1.1 and hardens the diagnostic control and deployment boundary for controlled two-Kaonic bench testing.

- Keeps ATAK forwarding independent from optional diagnostics startup.
- Uses a restrictive Unix datagram socket at `/run/kaonic-atak-plugin/diagnostics.sock` for local diagnostics control.
- Retains UDP diagnostics control only as an explicit compatibility/test option.
- Creates no diagnostic Reticulum destination or diagnostic radio announcements during ordinary bridge operation.
- Keeps unauthenticated mesh diagnostics behind the explicit trusted-test override.
- Clears diagnostic records on disable, expiration, restart, and fresh enable.
- Bounds diagnostic commands, replay state, record count, and parsed CoT identity fields.
- Removes peer hashes, callsigns, UIDs, and coordinates from default bridge logs.
- Pins the reviewed Reticulum dependency revision in workspace manifests.
- Adds a cargo-deny policy documenting remaining transitive advisory exceptions.
- Produces deterministic ZIP entry ordering and commit-time package timestamps.

Operational mesh diagnostics remain unsupported until signed management authorization and state synchronization are implemented.

## v0.1.1

Current package:

```text
Builds/KATAK-v0.1.1.zip
```

This build is the locally validated custom ATAK bridge baseline. It includes the default Kaonic ATAK bridge behavior from v0.1.0 and adds the safety, parsing, and packaging work needed for controlled field testing.

### Bridge behavior

- Carries ATAK Cursor-on-Target traffic over the Kaonic Reticulum radio mesh.
- Supports the standard ATAK multicast channels used by this project:

| Traffic | Multicast group | Port |
| --- | ---: | ---: |
| Situational awareness / CoT | `239.2.3.1` | `6969` |
| GeoChat / CoT | `224.10.10.1` | `17012` |

- Forwards valid CoT packets byte-for-byte after validation.
- Creates separate Reticulum destinations for the supported ATAK ports.
- Uses Reticulum announce data to auto-link matching ATAK bridge peers.
- Re-announces bridge destinations periodically so peers can reconnect during runtime.
- Sends a radio control keepalive while the service is running.

### Safety and validation

- Safe mode is the default forwarding mode.
- Local UDP payloads are parsed as UTF-8 CoT XML before entering the radio path.
- Remote Reticulum payloads are parsed before being rebroadcast to local ATAK multicast.
- Invalid XML, non-UTF-8 data, missing required CoT attributes, invalid numeric fields, and out-of-range coordinates are dropped in safe mode.
- Compatibility mode is available only through an explicit override:

```bash
KAONIC_ATAK_ALLOW_OPAQUE_FORWARDING=true
```

or:

```bash
kaonic-atak-plugin --allow-unvalidated-payloads
```

### Location tracking

- Reads CoT `uid`, event type, `how`, callsign, time metadata, and point data when present.
- Tracks latitude, longitude, HAE, CE, and LE values for location-bearing CoT events.
- Keeps local and remote location records separate.
- Bounds in-memory location state and prunes old records so long-running services do not grow without limit.
- Accepts valid non-location CoT events for forwarding without creating location records.

### Network isolation

- Binds ATAK multicast traffic to one selected local IPv4 interface.
- Supports explicit interface selection by name or IPv4 address.
- Reads interface overrides from command-line arguments or environment variables.
- Auto-detects only a single suitable `192.168.10.0/24` ATAK-facing address.
- Fails closed when automatic interface selection is missing or ambiguous.
- Sends remote ATAK multicast only through the selected ATAK-facing interface.

### Runtime configuration

- Uses `/kaonic-gateway.db` as the default gateway database path through the service file.
- Uses `192.168.10.1:9090` as the default Kaonic control endpoint.
- Runs radio module `0` by default in the packaged systemd service.
- Supports a named seed key for the plugin identity.
- Handles Ctrl-C and SIGTERM shutdown through a shared cancellation path.

### Build and packaging

- Built for the Kaonic ARMv7 target:

```text
armv7-unknown-linux-musleabihf
```

- Packaged as a Kaonic installer ZIP with:

```text
kaonic-plugin.toml
kaonic-atak-plugin.service
kaonic-atak-plugin
kaonic-atak-plugin.sha256
```

- Local validation completed with:

```bash
cargo fmt --check
cargo test -p kaonic-atak-plugin
cargo check -p kaonic-atak-plugin
cross build --release -p kaonic-atak-plugin --target armv7-unknown-linux-musleabihf
./scripts/package-plugin.sh armv7-unknown-linux-musleabihf
```

- The package was copied into `Builds` using the existing release naming pattern.

Physical installation, live ATAK ingress, and two-Kaonic radio testing are still separate validation steps and should be recorded before operational use.

## v0.1.0

Initial package:

```text
Builds/KATAK-v0.1.0.zip
```

This build represents the default Kaonic ATAK bridge feature set.

- Provides a Kaonic-side ATAK bridge service.
- Carries ATAK multicast traffic between a local ATAK network and the Kaonic radio mesh.
- Supports the project ATAK channels for CoT situational-awareness traffic and GeoChat traffic.
- Packages the bridge as a Kaonic plugin ZIP with a manifest, systemd service file, executable, and checksum file.
- Establishes the baseline release naming pattern used by later builds.
