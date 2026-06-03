# Build and Install

## Before you begin

The plugin is intended to run on the Kaonic's ARMv7 Linux environment. The deployment flow is:

1. Build the release binary for the Kaonic target.
2. Package the binary with its service file and plugin manifest.
3. Upload the ZIP through the Kaonic plugin installer.
4. Confirm that the service starts on the intended ATAK-facing network.

The current custom implementation should be treated as pre-deployment software until it has been built and tested on hardware.

## Target platform

The previously tested Kaonic reported the following architecture:

```text
armv7l
```

The repository is configured to build the plugin for:

```text
armv7-unknown-linux-musleabihf
```

## Build the plugin

From the repository root, run:

```bash
cross build --release -p kaonic-atak-plugin --target armv7-unknown-linux-musleabihf
```

Before packaging a release, also run the checks supported by the local toolchain:

```bash
cargo fmt --check
cargo test -p kaonic-atak-plugin
cargo check -p kaonic-atak-plugin
```

If the workspace requires the cross-build environment for dependency resolution, run the relevant check in that environment and record the result with the release notes.

## Create the plugin ZIP

The repository includes a packaging script:

```bash
./scripts/package-plugin.sh armv7-unknown-linux-musleabihf
```

The script expects the compiled binary at:

```text
target/armv7-unknown-linux-musleabihf/release/kaonic-atak-plugin
```

It creates:

```text
deploy/kaonic-atak-plugin/kaonic-atak-plugin.zip
```

The ZIP contains the files required by the Kaonic installer:

```text
kaonic-plugin.toml
kaonic-atak-plugin.service
kaonic-atak-plugin
kaonic-atak-plugin.sha256
```

## Runtime assumptions

The service is currently configured with the previously verified gateway database path:

```ini
Environment="KAONIC_GATEWAY_DB_PATH=/kaonic-gateway.db"
```

The default control endpoint used by the plugin is:

```text
192.168.10.1:9090
```

## Configure the local ATAK interface

The service should run only when the plugin can safely identify the local network used by ATAK. If the intended ATAK-facing address is not the single available address on `192.168.10.0/24`, add an explicit environment override to the deployed service before starting it:

```ini
Environment="KAONIC_ATAK_INTERFACE_IP=192.168.10.1"
```

Substitute the address assigned to the Kaonic on the network shared with the ATAK device.

After changing the unit file:

```bash
systemctl daemon-reload
systemctl restart kaonic-atak-plugin
```

## Installation check

After installation, review the service log before running an ATAK test:

```bash
systemctl status kaonic-atak-plugin
journalctl -u kaonic-atak-plugin -b
```

The startup log should identify one ATAK-facing interface and address. If interface selection fails, correct the configuration rather than enabling traffic on an arbitrary network.
