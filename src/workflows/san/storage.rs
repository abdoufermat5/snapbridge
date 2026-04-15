use log::{info, warn};
use serde_json::to_string_pretty;

use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{LoadedConfig, SanStorageConfig, StorageBackend, StorageConfig};
use crate::error::{AppError, Result};
use crate::shell::ShellRunner;
use crate::util::ontap_snapshot_name;

use super::super::nas::get_vms_by_storage;

pub async fn create_storage_snapshot<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    fsfreeze: bool,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;

    let mut frozen_vms = Vec::new();
    if fsfreeze {
        for vm in get_vms_by_storage(proxmox, storage_id).await? {
            if vm.status != "running" {
                info!(
                    "skipping fsfreeze for vm {} ({}): not running",
                    vm.vmid, vm.name
                );
                continue;
            }

            match proxmox
                .run_guest_agent_command(&vm.node, vm.vmid, "fsfreeze-freeze")
                .await
            {
                Ok(_) => frozen_vms.push(vm),
                Err(error) => warn!(
                    "fsfreeze failed for vm {} ({}): {}",
                    vm.vmid, vm.name, error
                ),
            }
        }
    }

    let snapshot_name = ontap_snapshot_name(&config.proxmox.timezone)?;
    let snapshot_comment = format!("Snapshot of Proxmox SAN storage {storage_id}");
    let result = ontap
        .create_snapshot(&volume, &snapshot_name, &snapshot_comment)
        .await;

    for vm in frozen_vms {
        if let Err(error) = proxmox
            .run_guest_agent_command(&vm.node, vm.vmid, "fsfreeze-thaw")
            .await
        {
            warn!(
                "fsfreeze-thaw failed for vm {} ({}): {}",
                vm.vmid, vm.name, error
            );
        }
    }

    result
}

pub async fn restore_storage_snapshot<P, O, S>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    shell: &S,
    storage_id: &str,
    snapshot: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
    S: ShellRunner,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
    ontap.restore_snapshot(&volume, snapshot).await?;

    for node in proxmox.list_nodes().await? {
        shell
            .ssh(
                &node.node,
                &san_config.ssh_user,
                "iscsiadm -m session --rescan",
            )
            .await?;
        shell
            .ssh(&node.node, &san_config.ssh_user, "pvscan --cache")
            .await?;
    }

    Ok(())
}

pub async fn delete_storage_snapshot<P, O>(
    config: &LoadedConfig,
    _: &P,
    ontap: &O,
    storage_id: &str,
    snapshot: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
    ontap.delete_snapshot(&volume.uuid, snapshot).await
}

pub async fn list_storage_snapshots<P, O>(
    config: &LoadedConfig,
    _: &P,
    ontap: &O,
    storage_id: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
    let snapshots = ontap.list_snapshots(&volume.uuid).await?;

    for snapshot in snapshots
        .into_iter()
        .filter(|snapshot| snapshot.name.starts_with("proxmox_snapshot_"))
    {
        println!(
            "Name: {}, Comment: {}",
            snapshot.name,
            snapshot.comment.unwrap_or_default()
        );
    }

    Ok(())
}

pub async fn show_storage<P, O>(
    config: &LoadedConfig,
    _: &P,
    ontap: &O,
    storage_id: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
    let lun = ontap.get_lun_by_name(&san_config.lun_path).await?;
    let mappings = ontap.list_lun_maps().await?;

    println!("=== Volume Info ===");
    println!(
        "{}",
        to_string_pretty(&ontap.get_volume_detail(&volume.uuid).await?)?
    );

    println!();
    println!("=== LUN Info ===");
    println!(
        "{}",
        to_string_pretty(&ontap.get_lun_detail(&lun.uuid).await?)?
    );

    println!();
    println!("=== iGroups (discovered from LUN mappings) ===");
    for mapping in mappings
        .iter()
        .filter(|mapping| mapping.lun_name.as_deref() == Some(&san_config.lun_path))
    {
        if let Some(igroup_name) = &mapping.igroup_name {
            println!();
            println!("--- {igroup_name} ---");
            println!(
                "{}",
                to_string_pretty(&ontap.get_igroup_detail(igroup_name).await?)?
            );
        }
    }

    println!();
    println!("=== LUN Mappings ===");
    for mapping in mappings.into_iter().filter(|mapping| {
        mapping
            .lun_name
            .as_deref()
            .is_some_and(|name| name.contains(&san_config.volume_name))
    }) {
        println!("{}", to_string_pretty(&mapping.raw)?);
    }

    Ok(())
}

fn require_san_config<'a>(
    config: &'a LoadedConfig,
    storage_id: &str,
) -> Result<&'a SanStorageConfig> {
    match config.require_backend(storage_id, StorageBackend::San)? {
        StorageConfig::San(config) => Ok(config),
        StorageConfig::Nas(_) => Err(AppError::Config(format!(
            "storage `{storage_id}` is not configured as SAN"
        ))),
    }
}
