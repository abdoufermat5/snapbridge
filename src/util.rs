use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::error::{AppError, Result};
use crate::models::{NasDisk, VmConfig};

pub fn timezone(name: &str) -> Result<Tz> {
    name.parse::<Tz>()
        .map_err(|_| AppError::Config(format!("invalid timezone `{name}`")))
}

pub fn zoned_now(timezone_name: &str) -> Result<DateTime<Tz>> {
    Ok(Utc::now().with_timezone(&timezone(timezone_name)?))
}

pub fn ontap_snapshot_name(timezone_name: &str) -> Result<String> {
    Ok(format!(
        "proxmox_snapshot_{}",
        zoned_now(timezone_name)?.format("%Y-%m-%d_%H:%M:%S%z")
    ))
}

pub fn pve_snapshot_name(timezone_name: &str) -> Result<String> {
    Ok(format!(
        "pve_ontap_{}",
        zoned_now(timezone_name)?.format("%Y%m%d%H%M%S")
    ))
}

pub fn extract_nas_vm_disks(config: &VmConfig) -> Vec<NasDisk> {
    let mut disks = Vec::new();

    for (key, value) in config {
        let Some(value) = value.as_str() else {
            continue;
        };

        if !matches_interface(key) || value.contains("cdrom") {
            continue;
        }

        if !(value.contains("qcow2") || value.contains("raw") || value.contains("vmdk")) {
            continue;
        }

        let Some((storage, remainder)) = value.split_once(':') else {
            continue;
        };
        let disk_path = remainder.split(',').next().unwrap_or_default().to_owned();
        disks.push(NasDisk {
            storage: storage.to_owned(),
            disk_path,
        });
    }

    disks
}

pub fn vm_uses_storage_scsi_only(config: &VmConfig, storage_name: &str) -> bool {
    config.iter().any(|(key, value)| {
        key.starts_with("scsi")
            && value.as_str().is_some_and(|value| {
                value.starts_with(&format!("{storage_name}:")) && !value.contains("cdrom")
            })
    })
}

fn matches_interface(key: &str) -> bool {
    key.starts_with("ide") || key.starts_with("sata") || key.starts_with("scsi")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{extract_nas_vm_disks, ontap_snapshot_name, vm_uses_storage_scsi_only};

    #[test]
    fn snapshot_name_has_expected_prefix() {
        let snapshot = ontap_snapshot_name("Europe/Paris").expect("snapshot name should build");
        assert!(snapshot.starts_with("proxmox_snapshot_"));
    }

    #[test]
    fn extracts_only_supported_disk_entries() {
        let config = [
            (
                "scsi0".to_owned(),
                json!("NAS01:100/vm-100-disk-0.raw,discard=on"),
            ),
            ("ide2".to_owned(), json!("NAS01:cloudinit,media=cdrom")),
            ("sata0".to_owned(), json!("NAS02:100/vm-100-disk-1.qcow2")),
            ("unused0".to_owned(), json!("NAS01:100/vm-100-disk-2.raw")),
        ]
        .into_iter()
        .collect();

        let disks = extract_nas_vm_disks(&config);
        assert_eq!(disks.len(), 2);
        assert!(
            disks.iter().any(|disk| {
                disk.storage == "NAS01" && disk.disk_path == "100/vm-100-disk-0.raw"
            })
        );
        assert!(disks.iter().any(|disk| {
            disk.storage == "NAS02" && disk.disk_path == "100/vm-100-disk-1.qcow2"
        }));
        assert!(vm_uses_storage_scsi_only(&config, "NAS01"));
        assert!(!vm_uses_storage_scsi_only(&config, "SAN01"));
    }
}
