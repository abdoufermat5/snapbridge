# Proxsnap

`proxsnap` is a Rust CLI for managing Proxmox snapshots on NetApp ONTAP-backed storage.

It ports the behavior from `cloudsante-pve-ontap-snapshot` into a single binary with two operator-facing backends:
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

## CLI

Top-level help:

```bash
proxsnap --help
```

Main commands:

```bash
proxsnap nas vm create --vm 100 --suspend
proxsnap nas storage create --storage NAS01 --fsfreeze
proxsnap nas storage list --storage NAS01
proxsnap nas storage mount --storage NAS01 --snapshot proxmox_snapshot_2026-04-14_02:00:00+0200

proxsnap san storage create --storage SAN01 --fsfreeze
proxsnap san storage restore --storage SAN01 --snapshot proxmox_snapshot_2026-04-14_02:15:00+0200
proxsnap san storage show --storage SAN01
```

## Config

The CLI reads `./proxsnap.toml` by default. Override it with `--config <path>`.

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
```

### Config Notes

- `proxmox.host` may be a hostname or full URL. If you pass a hostname, `https://<host>:8006` is assumed.
- NAS entries do not need an ONTAP volume name in config. The volume is derived from the Proxmox storage export path.
- SAN entries require `volume_name` and `lun_path`.
- SAN restore uses SSH to each Proxmox node and runs:
  - `iscsiadm -m session --rescan`
  - `pvscan --cache`

## Behavior Notes

- NAS VM snapshots use ONTAP file cloning for eligible VM disks.
- NAS storage `create --fsfreeze` creates temporary Proxmox VM snapshots before taking the ONTAP snapshot, then removes them.
- SAN storage `create --fsfreeze` uses the QEMU guest agent directly with `fsfreeze-freeze` / `fsfreeze-thaw`.
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
- `workflows/` contains the NAS and SAN behavior layers
- `config.rs`, `models.rs`, `util.rs`, and `error.rs` contain shared support code

## Verification

Run the test suite:

```bash
cargo test
```

At the moment the tests are mock-driven unit/integration-style checks around workflow behavior. They lock the current command flow and refactor safety, but they do not replace a real lab validation against your Proxmox and ONTAP APIs.
