use crate::clients::ontap::{OntapApi, ReqwestOntapClient};
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{LoadedConfig, StorageBackend, StorageConfig};
use crate::error::{AppError, Result};
use crate::logger::ProgressLogger;
use crate::models::FileCloneRequest;
use crate::util::{extract_nas_vm_disks, ontap_snapshot_name};

use super::discovery::{find_vm, resolve_nas_volume_name, wait_for_task};

pub async fn create_vm_snapshot<P>(
    config: &LoadedConfig,
    proxmox: &P,
    vmid: u32,
    suspend: bool,
    shutdown: bool,
) -> Result<()>
where
    P: ProxmoxApi,
{
    create_vm_snapshot_with_factory(config, proxmox, vmid, suspend, shutdown, |storage_id| {
        let storage = config.require_backend(storage_id, StorageBackend::Nas)?;
        let StorageConfig::Nas(settings) = storage else {
            return Err(AppError::Config(format!(
                "storage `{storage_id}` is not configured for NAS"
            )));
        };
        ReqwestOntapClient::new(settings.shared())
    })
    .await
}

pub(crate) async fn create_vm_snapshot_with_factory<P, O, F>(
    config: &LoadedConfig,
    proxmox: &P,
    vmid: u32,
    suspend: bool,
    shutdown: bool,
    ontap_factory: F,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
    F: Fn(&str) -> Result<O>,
{
    let vm = find_vm(proxmox, vmid).await?;
    let progress = ProgressLogger::new("nas", "vm-snapshot", vm.vmid.to_string());
    progress.start(format!(
        "starting NAS VM snapshot for VM {} ({})",
        vm.vmid, vm.name
    ));
    let mut current_status = vm.status.clone();

    if suspend {
        progress.step(format!("suspending VM {} ({})", vm.vmid, vm.name));
        let task = proxmox.suspend_vm(&vm.node, vm.vmid).await?;
        wait_for_task(proxmox, &vm.node, &task).await?;
        current_status = proxmox.get_vm_status(&vm.node, vm.vmid).await?.status;
        progress.step(format!(
            "VM {} status after suspend: {current_status}",
            vm.vmid
        ));
    }

    if shutdown {
        progress.step(format!("shutting down VM {} ({})", vm.vmid, vm.name));
        let task = proxmox.shutdown_vm(&vm.node, vm.vmid).await?;
        wait_for_task(proxmox, &vm.node, &task).await?;
        current_status = proxmox.get_vm_status(&vm.node, vm.vmid).await?.status;
        progress.step(format!(
            "VM {} status after shutdown: {current_status}",
            vm.vmid
        ));
    }

    if current_status != "stopped" {
        progress.warn("creating snapshot of a running VM; the result might be inconsistent");
    }

    progress.step("loading VM disk configuration");
    let vm_config = proxmox.get_vm_config(&vm.node, vm.vmid).await?;
    let timestamp = ontap_snapshot_name(&config.proxmox.timezone)?;
    let disks = extract_nas_vm_disks(&vm_config);
    progress.step(format!("found {} NAS disk(s) to clone", disks.len()));

    for disk in disks {
        progress.step(format!(
            "preparing disk `{}` on storage `{}`",
            disk.disk_path, disk.storage
        ));
        config.require_backend(&disk.storage, StorageBackend::Nas)?;
        let ontap = ontap_factory(&disk.storage)?;
        let volume_name = resolve_nas_volume_name(proxmox, &disk.storage).await?;
        progress.step(format!(
            "loading ONTAP volume `{volume_name}` for storage `{}`",
            disk.storage
        ));
        let volume = ontap.get_volume_by_name(&volume_name).await?;
        let (vm_dir, filename) = disk.disk_path.rsplit_once('/').ok_or_else(|| {
            AppError::Unexpected(format!("invalid disk path `{}`", disk.disk_path))
        })?;
        let (stem, extension) = match filename.rsplit_once('.') {
            Some((stem, ext)) => (stem, format!(".{ext}")),
            None => (filename, String::new()),
        };
        let snapshot_filename = format!("{stem}-snapshot-{timestamp}{extension}");

        progress.step(format!(
            "cloning `{}` to `{snapshot_filename}` on storage `{}`",
            disk.disk_path, disk.storage
        ));
        ontap
            .clone_file(&FileCloneRequest {
                volume_name: volume.name.clone(),
                volume_uuid: volume.uuid.clone(),
                source_path: format!("images/{}", disk.disk_path),
                destination_path: format!("images/{vm_dir}/{snapshot_filename}"),
                overwrite_destination: false,
            })
            .await?;
    }

    if suspend || shutdown {
        progress.step(format!("starting VM {} ({})", vm.vmid, vm.name));
        let task = proxmox.start_vm(&vm.node, vm.vmid).await?;
        wait_for_task(proxmox, &vm.node, &task).await?;
    }

    progress.success(format!(
        "NAS VM snapshot completed for VM {} ({})",
        vm.vmid, vm.name
    ));
    Ok(())
}
