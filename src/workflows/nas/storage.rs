use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{LoadedConfig, StorageBackend};
use crate::display::{OutputFormat, SnapshotRow};
use crate::error::{AppError, Result};
use crate::logger::ProgressLogger;
use crate::models::{FlexCloneRequest, ProxmoxStorage, VmRef};
use crate::util::{ontap_snapshot_name, pve_snapshot_name};

use super::discovery::{get_vms_by_storage, resolve_nas_volume_name, wait_for_task};

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
    let progress = ProgressLogger::new("nas", "snapshot", storage_id);
    progress.start("starting NAS storage snapshot");
    progress.step("checking storage backend");
    config.require_backend(storage_id, StorageBackend::Nas)?;

    progress.step("resolving Proxmox storage export to ONTAP volume");
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    progress.step(format!("loading ONTAP volume `{volume_name}`"));
    let volume = ontap.get_volume_by_name(&volume_name).await?;

    let mut pve_snapshots: Vec<(VmRef, String)> = Vec::new();
    if fsfreeze {
        let snapname = pve_snapshot_name(&config.proxmox.timezone)?;
        progress.step(format!(
            "discovering VMs that use storage `{storage_id}` before fsfreeze"
        ));
        let vms = get_vms_by_storage(proxmox, storage_id).await?;
        progress.step(format!("found {} VM(s) using storage", vms.len()));

        for vm in vms {
            if vm.status != "running" {
                progress.skip(format!(
                    "VM {} ({}) is {}; fsfreeze only applies to running VMs",
                    vm.vmid, vm.name, vm.status
                ));
                continue;
            }

            progress.step(format!(
                "creating temporary Proxmox snapshot `{snapname}` for VM {} ({})",
                vm.vmid, vm.name
            ));
            let task = proxmox
                .create_vm_snapshot(&vm.node, vm.vmid, &snapname, "snapbridge fsfreeze")
                .await?;
            let exitstatus = wait_for_task(proxmox, &vm.node, &task).await?;
            if exitstatus != "OK" {
                progress.warn(format!(
                    "fsfreeze snapshot for vm {} ({}) failed: {}",
                    vm.vmid, vm.name, exitstatus
                ));
            } else {
                progress.step(format!(
                    "temporary Proxmox snapshot `{snapname}` ready for VM {} ({})",
                    vm.vmid, vm.name
                ));
                pve_snapshots.push((vm, snapname.clone()));
            }
        }
    } else {
        progress.step("fsfreeze disabled; skipping temporary Proxmox snapshots");
    }

    let snapshot_name = ontap_snapshot_name(&config.proxmox.timezone)?;
    let snapshot_comment = format!("Snapshot of Proxmox NAS storage {storage_id}");
    progress.step(format!(
        "creating ONTAP snapshot `{snapshot_name}` on volume `{}`",
        volume.name
    ));
    let create_result = ontap
        .create_snapshot(&volume, &snapshot_name, &snapshot_comment)
        .await;

    match &create_result {
        Ok(()) => progress.step("ONTAP snapshot create request completed"),
        Err(error) => progress.warn(format!(
            "ONTAP snapshot create failed; cleaning up temporary snapshots before returning: {error}"
        )),
    }

    if pve_snapshots.is_empty() {
        progress.step("no temporary Proxmox snapshots to clean up");
    } else {
        progress.step(format!(
            "cleaning up {} temporary Proxmox snapshot(s)",
            pve_snapshots.len()
        ));
    }

    for (vm, snapname) in pve_snapshots {
        progress.step(format!(
            "deleting temporary Proxmox snapshot `{snapname}` for VM {} ({})",
            vm.vmid, vm.name
        ));
        match proxmox
            .delete_vm_snapshot(&vm.node, vm.vmid, &snapname)
            .await
        {
            Ok(task) => {
                if let Err(error) = wait_for_task(proxmox, &vm.node, &task).await {
                    progress.warn(format!(
                        "failed to delete fsfreeze snapshot {} for vm {} ({}): {}",
                        snapname, vm.vmid, vm.name, error
                    ));
                }
            }
            Err(error) => progress.warn(format!(
                "failed to delete fsfreeze snapshot {} for vm {} ({}): {}",
                snapname, vm.vmid, vm.name, error
            )),
        }
    }

    match create_result {
        Ok(()) => {
            progress.success(format!("snapshot `{snapshot_name}` created"));
            Ok(())
        }
        Err(error) => {
            progress.failed(format!("snapshot `{snapshot_name}` failed: {error}"));
            Err(error)
        }
    }
}

pub async fn restore_storage_snapshot<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    snapshot: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;
    ontap.restore_snapshot(&volume, snapshot).await
}

pub async fn delete_storage_snapshot<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    snapshot: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;
    ontap.delete_snapshot(&volume.uuid, snapshot).await
}

pub async fn list_storage_snapshots<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    output: OutputFormat,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let snapshots = storage_snapshot_rows(config, proxmox, ontap, storage_id).await?;
    crate::display::print_snapshots(output, &snapshots)
}

pub async fn storage_snapshot_rows<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
) -> Result<Vec<SnapshotRow>>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;
    let snapshots = ontap.list_snapshots(&volume.uuid).await?;

    Ok(snapshots
        .into_iter()
        .filter(|snapshot| snapshot.name.starts_with("proxmox_snapshot_"))
        .map(|snapshot| {
            SnapshotRow::new(
                storage_id,
                snapshot.name,
                snapshot.comment.unwrap_or_default(),
            )
        })
        .collect())
}

pub async fn mount_storage_snapshot<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    snapshot: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;
    let clone_name = format!("{}_clone", volume.name);
    let nas_path = format!("/{clone_name}");

    ontap
        .create_flexclone(&FlexCloneRequest {
            parent_volume_name: volume.name.clone(),
            parent_snapshot_name: snapshot.to_owned(),
            clone_name: clone_name.clone(),
            svm_name: volume.svm_name.clone(),
            nas_path: nas_path.clone(),
        })
        .await?;

    let current_storage = proxmox.get_storage(storage_id).await?;
    proxmox
        .create_storage(
            &format!("{storage_id}-CLONE"),
            &ProxmoxStorage {
                storage_type: current_storage.storage_type,
                server: current_storage.server,
                content: current_storage.content,
                export: nas_path,
            },
        )
        .await
}

pub async fn unmount_storage_snapshot<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;

    if !volume.is_flexclone {
        return Err(AppError::Unexpected(format!(
            "storage `{storage_id}` is not backed by a FlexClone volume"
        )));
    }

    proxmox.delete_storage(storage_id).await?;
    ontap.delete_volume(&volume.uuid, true).await
}

pub async fn show_storage<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
    output: OutputFormat,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    config.require_backend(storage_id, StorageBackend::Nas)?;
    let volume_name = resolve_nas_volume_name(proxmox, storage_id).await?;
    let volume = ontap.get_volume_by_name(&volume_name).await?;
    let detail = ontap.get_volume_detail(&volume.uuid).await?;
    crate::display::print_detail(output, "Volume Info", &detail)
}
