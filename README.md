# Proxsnap

`proxsnap` is a Rust CLI for managing Proxmox snapshots on NetApp ONTAP-backed storage.

It ports some behavior from `pve-ontap-snapshot` (https://github.com/credativ/pve-ontap-snapshot) into a single binary with two operator-facing backends:
- `nas` for ONTAP-backed NAS/NFS storage
- `san` for ONTAP-backed iSCSI/LVM storage

## Status

Current implementation covers:
- `proxsnap nas vm create`
- `proxsnap nas storage create|restore|delete|list|mount|unmount|show`
- `proxsnap san storage create|restore|delete|list|show`

Not included yet:
- retention cleanup helpers
- cron packaging
- live integration coverage against a real Proxmox/ONTAP environment

## Build

```bash
cargo build
```

Run the CLI:

```bash
cargo run -- --help
```

Build a local Debian package when `cargo-deb` is installed:

```bash
cargo install cargo-deb --version 3.6.3 --locked
cargo deb -- --locked
```

Release builds are automated by GitHub Actions. Pushing a version tag such as `v0.1.0` runs tests, lints, builds `amd64` and `arm64` Debian packages, generates `SHA256SUMS`, and uploads the files to the GitHub Release for that tag.

## Install

Install the latest release on Debian-compatible `amd64` or `arm64` systems:

```bash
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/proxsnap/main/install.sh | bash
```

Install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/proxsnap/main/install.sh | PROXSNAP_VERSION=v0.1.0 bash
```

The installer downloads the matching `.deb` package from `https://github.com/abdoufermat5/proxsnap/releases`, verifies it against the release `SHA256SUMS`, and installs it with `apt-get` or `dpkg`.

## CLI

Top-level help:

```bash
proxsnap --help
```

Main commands:

```bash
proxsnap nas vm create --vm 100 --suspend
proxsnap nas storage create --storage NAS01 --fsfreeze
proxsnap nas storage list
proxsnap nas storage list --storage NAS01
proxsnap nas storage list --output json
proxsnap nas storage mount --storage NAS01 --snapshot proxmox_snapshot_2026-04-14_02:00:00+0200

proxsnap san storage create --storage SAN01 --fsfreeze
proxsnap san storage list
proxsnap san storage list --storage SAN01
proxsnap san storage list --output json
proxsnap san storage restore --storage SAN01 --snapshot proxmox_snapshot_2026-04-14_02:15:00+0200
proxsnap san storage show --storage SAN01
proxsnap san storage show --storage SAN01 --output json

proxsnap schedule list
proxsnap schedule run daily
proxsnap schedule create daily
proxsnap schedule delete daily
```

### Output

Human-readable table output is the default:

```bash
proxsnap nas storage list
proxsnap san storage list
```

Use `--output json` when piping to scripts or other tools:

```bash
proxsnap nas storage list --output json
proxsnap san storage show --storage SAN01 --output json
```

`nas storage list` and `san storage list` list all configured storage entries for their backend when `--storage` is omitted. Add `--storage <id>` to limit the output to one configured storage.

### Logging

The default log level is `info`, so snapshot creation prints progress as it runs. Progress logs use a shared format:

```text
[INFO] [san snapshot:SAN01] start: starting SAN storage snapshot
[INFO] [san snapshot:SAN01] step: discovering VMs that use storage `SAN01` before fsfreeze
[INFO] [san snapshot:SAN01] step: creating ONTAP snapshot `proxmox_snapshot_...` on volume `san_vol1`
[INFO] [san snapshot:SAN01] done: snapshot `proxmox_snapshot_...` created
```

Use `--log-level warn` for quieter output or `--log-level debug` to include HTTP response debug logs:

```bash
proxsnap --log-level warn nas storage create --storage NAS01
proxsnap --log-level debug san storage create --storage SAN01 --fsfreeze
```

## Config

The installed package reads `/etc/proxsnap/proxsnap.toml` by default. The Debian package installs a copy of `proxsnap.example.toml` there with mode `600`.

Edit it after installation:

```bash
sudo nano /etc/proxsnap/proxsnap.toml
sudo chmod 600 /etc/proxsnap/proxsnap.toml
```

Override the config path with `--config <path>` when needed.

Example:

```toml
[proxmox]
host = "pve.example.local"
user = "root@pam"
token_name = "proxsnap"
token_value = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
verify_ssl = false
timezone = "Europe/Paris"

[storage.NAS01]
backend = "nas"
ontap_host = "ontap-mgmt.example.local"
ontap_user = "admin"
ontap_password = "secret"
verify_ssl = false

[storage.SAN01]
backend = "san"
ontap_host = "ontap-mgmt.example.local"
ontap_user = "admin"
ontap_password = "secret"
verify_ssl = false
volume_name = "san_vol1"
lun_path = "/vol/san_vol1/lun0"
ssh_user = "root"

[schedule.daily]
storages = ["NAS01", "SAN01"]
fsfreeze = true
keep_last = 7
max_age = "30d"
```

### Config Notes

- `proxmox.host` may be a hostname or full URL. If you pass a hostname, `https://<host>:8006` is assumed.
- NAS entries do not need an ONTAP volume name in config. The volume is derived from the Proxmox storage export path.
- SAN entries require `volume_name` and `lun_path`.
- Schedule entries target explicit storage IDs. `fsfreeze` defaults to `false`.
- Schedule retention supports `keep_last`, `max_age`, or both. `max_age` accepts `s`, `m`, `h`, `d`, and `w` suffixes, for example `30d`.
- SAN restore uses SSH to each Proxmox node and runs:
  - `iscsiadm -m session --rescan`
  - `pvscan --cache`

## Scheduling

Schedules live in `/etc/proxsnap/proxsnap.toml` under `[schedule.<name>]`.

Run a schedule manually:

```bash
proxsnap schedule run daily
```

Run only one phase:

```bash
proxsnap schedule create daily
proxsnap schedule delete daily
```

The Debian package installs reusable systemd units:
- `/lib/systemd/system/proxsnap-schedule@.service`
- `/lib/systemd/system/proxsnap-schedule@.timer`

Enable the packaged daily timer for a schedule named `daily`:

```bash
sudo systemctl enable --now proxsnap-schedule@daily.timer
sudo systemctl status proxsnap-schedule@daily.timer
journalctl -u proxsnap-schedule@daily.service
```

The timer runs at `02:00` by default. Override timing with a systemd drop-in for each schedule:

```bash
sudo systemctl edit proxsnap-schedule@daily.timer
```

Example override:

```ini
[Timer]
OnCalendar=
OnCalendar=*-*-* 03:30:00
```

## Behavior Notes

- NAS VM snapshots use ONTAP file cloning for eligible VM disks.
- NAS storage `create --fsfreeze` creates temporary Proxmox VM snapshots before taking the ONTAP snapshot, then removes them.
- SAN storage `create --fsfreeze` uses the QEMU guest agent directly with `fsfreeze-freeze` / `fsfreeze-thaw`.
- Snapshot creation logs each major phase: config/backend checks, VM discovery, freeze/snapshot/thaw or temporary snapshot cleanup, and final success/failure.
- Scheduled deletion only removes snapshots whose names start with `proxmox_snapshot_` and contain a parseable Proxsnap timestamp.
- NAS `mount` creates a FlexClone volume and registers `<storage>-CLONE` in Proxmox.
- VM disk snapshots still keep the same known limitation as the Python version: Proxmox does not automatically rescan and display them as attached snapshots.

## Project Layout

Key folders:

```text
src/
  clients/
    ontap/
    proxmox/
  workflows/
    nas/
    san/
```

Rough responsibilities:
- `clients/` contains the API traits plus the reqwest-backed HTTP implementations
- `display.rs` contains shared table and JSON output rendering
- `logger.rs` contains shared CLI logging and progress messages
- `workflows/` contains the NAS and SAN behavior layers
- `config.rs`, `models.rs`, `util.rs`, and `error.rs` contain shared support code

## Packaging

Debian package metadata lives in `Cargo.toml` under `[package.metadata.deb]`.

The generated package installs:
- `/usr/bin/proxsnap`
- `/etc/proxsnap/proxsnap.toml`
- `/lib/systemd/system/proxsnap-schedule@.service`
- `/lib/systemd/system/proxsnap-schedule@.timer`
- `/usr/share/doc/proxsnap/README.md`
- `/usr/share/doc/proxsnap/examples/proxsnap.toml`

The release workflow is `.github/workflows/debian-release.yml`. It only runs for tags matching `v*`, requires the tag version to match `Cargo.toml`, and builds on Ubuntu 22.04 to keep the generated package compatible with Debian 12 / Proxmox 8 era `libc6` versions.

The root `install.sh` script is designed for `curl | bash` installation from GitHub Releases. Set `PROXSNAP_VERSION` to pin a specific tag; omit it to install the latest release.

## Verification

Run the test suite:

```bash
cargo test
```

Run the same lint gate used by CI:

```bash
cargo clippy --locked --all-targets -- -D warnings
```

At the moment the tests are mock-driven unit/integration-style checks around workflow behavior. They lock the current command flow and refactor safety, but they do not replace a real lab validation against your Proxmox and ONTAP APIs.
