# Kaonic ATAK Plugin

A custom Kaonic-hosted interface for transporting ATAK Cursor-on-Target and
GeoChat multicast traffic across the Kaonic Reticulum radio mesh.

This repository is separate from Beechat's upstream `kaonic-gateway`
repository. It is intended to become a Kaonic-side plugin/service, not an
Android ATAK APK plugin.

## Project Status

This repository is in the design/baseline stage. The immediate goal is to
document the known-good upstream ATAK bridge behavior before implementing the
custom plugin in this repository.

| Capability                                      | Status      |
| ----------------------------------------------- | ----------- |
| Upstream ATAK bridge identified and reviewed    | Complete    |
| Upstream bridge built for Kaonic ARMv7          | Complete    |
| Upstream bridge installed on one Kaonic         | Complete    |
| Local multicast ingress smoke test              | Passed      |
| Forwarding into Reticulum path                  | Passed      |
| Real ATAK phone traffic test                    | Pending     |
| Two-Kaonic end-to-end radio test                | Pending     |
| Custom plugin implementation in this repository | Not started |

## Intended Architecture

```text
ATAK Android Device
        │
        │ UDP multicast over local Kaonic network
        ▼
Kaonic ATAK Plugin
        │
        │ Reticulum over Kaonic radio transport
        ▼
Remote Kaonic ATAK Plugin
        │
        │ UDP multicast over local Kaonic network
        ▼
Remote ATAK Android Device
```

The plugin runs on the Kaonic device. It bridges local ATAK multicast traffic
into the Kaonic Reticulum radio mesh and emits received remote traffic back
onto the local ATAK multicast network.

It is separate from any future Android ATAK UI plugin. A future Android-side
plugin could be useful, but it is not part of this baseline repository scope.

## Baseline ATAK Traffic Channels

| Traffic Type | Multicast Group |    Port | Purpose                                             |
| ------------ | --------------: | ------: | --------------------------------------------------- |
| SA / CoT     |     `239.2.3.1` |  `6969` | Position, marker, and situational-awareness traffic |
| GeoChat      |   `224.10.10.1` | `17012` | Chat-related traffic                                |

Stage 0 should preserve these values unchanged for behavior-equivalence
testing against the upstream bridge.

## Upstream Reference Implementation

The baseline reference implementation exists in the upstream repository at:

```text
kaonic-atak-bridge/
```

Observed/source-confirmed responsibilities:

- Receive local ATAK multicast UDP traffic. Source: upstream
  `kaonic-atak-bridge/src/main.rs`.
- Forward received packets into Reticulum. Source: upstream
  `kaonic-atak-bridge/src/main.rs`.
- Advertise bridge destinations for supported ATAK channels. Source: upstream
  `kaonic-atak-bridge/src/main.rs`.
- Discover and link to compatible remote bridge destinations. Source: upstream
  `kaonic-atak-bridge/src/main.rs`.
- Forward remote Reticulum payloads back onto local ATAK multicast. Source:
  upstream `kaonic-atak-bridge/src/main.rs`.

The upstream bridge package metadata and service definition are in:

```text
kaonic-atak-bridge/Cargo.toml
kaonic-atak-bridge/kaonic-plugin.toml
kaonic-atak-bridge/kaonic-atak-bridge.service
```

The Kaonic plugin installer behavior is implemented under:

```text
kaonic-installer/
```

## Current Verified Test Status

Known device context:

```text
Hardware tested: Kaonic 1S
Device architecture: armv7l
Fresh OS image tested: ST OpenSTLinux / Yocto, VERSION_ID=5.0.3-snapshot-20260411
Observed Kaonic communication control endpoint: 192.168.10.1:9090
Observed gateway database path: /kaonic-gateway.db
```

The upstream ATAK bridge was manually built, packaged, installed, and tested on
one Kaonic device. Verified behavior:

- Plugin installation through the Kaonic dashboard succeeded.
- The bridge systemd service started successfully.
- The bridge joined multicast groups `239.2.3.1:6969` and
  `224.10.10.1:17012`.
- Local multicast test packets from a connected computer were received by the
  bridge.
- Service logs showed `udp -> rns`, confirming local multicast ingress and
  forwarding into the Reticulum path.

Limitations:

- End-to-end radio delivery has not been verified because only one Kaonic unit
  was available.
- Remote ATAK reception of position, markers, or chat has not been verified.
- Reliability, bandwidth behavior, filtering, and field performance have not
  been tested.
- Real ATAK phone packet ingress remains pending unless separate verified test
  results are added to this repository.

## Upstream Background

Verified timeline from the upstream `kaonic-gateway` git history:

```text
2026-03-18 — The ATAK bridge was first introduced in commit 24371ccb:
             chore: add atak bridge over reticulum

2026-03-18 — ATAK/Reticulum flow was corrected in commit d8ff138b:
             feat: correct atak reticulum flow

2026-04-11 — The prior bridge source was removed during the Leptos/gateway
             refactor in commit 237182c8, while ATAK-related gateway UI rows
             were added.

2026-04-26 — The current standalone ATAK plugin/service was reintroduced in
             commit e22746cd:
             feat: add ldpc to reticulum interface
```

The tested fresh OS image is dated `20260411`. That explains why it contained
ATAK-related gateway UI strings but did not contain the standalone
`kaonic-atak-bridge.service` or active multicast listeners by default.

## Runtime Requirements

Currently known runtime requirements:

- Kaonic 1S hardware.
- ARMv7-compatible plugin binary.
- `kaonic-commd.service` running.
- `kaonic-gateway.service` available for gateway settings/database access.
- Kaonic control endpoint reachable at `192.168.10.1:9090`.
- Gateway database available at `/kaonic-gateway.db`.
- ATAK phone or test client connected to the local Kaonic network.

The custom service definition should explicitly set:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
```

This absolute path was verified on the tested device. The upstream bridge
service definition did not explicitly set it.

## Planned Plugin Package Structure

The intended Kaonic installer ZIP baseline is:

```text
kaonic-atak-plugin.zip
├── kaonic-plugin.toml
├── kaonic-atak-plugin.service
├── kaonic-atak-plugin
└── kaonic-atak-plugin.sha256
```

This packaging structure is based on the successfully installed upstream bridge
package and must be confirmed against `kaonic-installer/` before the first
custom package is finalized.

## Development Roadmap

```text
Stage 0 — Behavior-equivalent bridge baseline
Reproduce the existing multicast ⇄ Reticulum bridge behavior with no new features.

Stage 1 — Reliability and configuration
Make interface selection, database path, radio module, logging, and reconnect handling configurable and robust.

Stage 2 — Diagnostics and observability
Expose packet counters, active peers, bridge health, link state, and errors.

Stage 3 — Kaonic-hosted ATAK interface
Add a management/status interface suitable for integration with the Kaonic workflow.

Stage 4 — End-to-end validation
Validate actual ATAK SA, markers, and GeoChat using two Kaonic radios and two ATAK devices.
```

## Known Improvement Areas

| Issue                                         | Why It Matters                                            | Priority |
| --------------------------------------------- | --------------------------------------------------------- | -------- |
| Hard-coded local subnet/interface detection   | May fail with different local network layouts             | High     |
| Retransmission on all non-loopback interfaces | May emit multicast traffic onto unintended interfaces     | High     |
| Radio daemon reconnect behavior               | Service may appear active while forwarding is unavailable | High     |
| Packet-by-packet informational logging        | May create excessive logs during normal ATAK use          | Medium   |
| No externally visible metrics or health API   | Limits diagnostics and future interface work              | Medium   |
| No completed two-radio end-to-end test        | Prevents operational validation                           | High     |

## Completed Test Record

A connected computer transmitted UDP multicast test packets to both ATAK bridge
multicast groups. The manually installed upstream bridge joined both groups and
logged `udp -> rns` after receiving the packets, confirming local multicast
ingress and handoff into the Reticulum transport path.

This test did not demonstrate remote RF delivery or remote ATAK reception.

## Repository Scope

This repository will contain the custom implementation and documentation. The
Beechat `kaonic-gateway` repository remains an upstream reference and dependency
source during early development.

Proposed repository structure, not yet implemented:

```text
Kaonic-ATAK-Plugin/
├── README.md
├── Cargo.toml
├── kaonic-plugin.toml
├── kaonic-atak-plugin.service
├── Cross.toml
├── scripts/
│   └── package-plugin.sh
└── src/
    ├── main.rs
    ├── bridge.rs
    ├── config.rs
    ├── multicast.rs
    ├── reticulum_transport.rs
    └── metrics.rs
```

## Next Steps

- [ ] Complete source extraction from the upstream ATAK bridge implementation.
- [ ] Decide how this standalone repository will reference or vendor required Kaonic gateway/Reticulum dependencies.
- [ ] Define the Stage 0 plugin package and service names.
- [ ] Implement the behavior-equivalent bridge baseline.
- [ ] Build for Kaonic ARMv7.
- [ ] Package and install through the Kaonic Plugins page.
- [ ] Validate real ATAK phone packet ingress.
- [ ] Perform two-Kaonic end-to-end testing.
