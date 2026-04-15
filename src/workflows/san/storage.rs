use serde::Serialize;
use serde_json::{Value, json};

use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{LoadedConfig, SanStorageConfig, StorageBackend, StorageConfig};
use crate::display::{DetailSection, OutputFormat, SnapshotRow};
use crate::error::{AppError, Result};
use crate::logger::ProgressLogger;
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
    let progress = ProgressLogger::new("san", "snapshot", storage_id);
    progress.start("starting SAN storage snapshot");
    progress.step("checking storage backend and loading SAN config");
    let san_config = require_san_config(config, storage_id)?;

    progress.step(format!("loading ONTAP volume `{}`", san_config.volume_name));
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;

    let mut frozen_vms = Vec::new();
    if fsfreeze {
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
                "freezing filesystem for VM {} ({}) with guest agent",
                vm.vmid, vm.name
            ));
            match proxmox
                .run_guest_agent_command(&vm.node, vm.vmid, "fsfreeze-freeze")
                .await
            {
                Ok(_) => {
                    progress.step(format!("VM {} ({}) frozen", vm.vmid, vm.name));
                    frozen_vms.push(vm);
                }
                Err(error) => progress.warn(format!(
                    "fsfreeze failed for vm {} ({}): {}",
                    vm.vmid, vm.name, error
                )),
            }
        }
    } else {
        progress.step("fsfreeze disabled; skipping guest filesystem freeze");
    }

    let snapshot_name = ontap_snapshot_name(&config.proxmox.timezone)?;
    let snapshot_comment = format!("Snapshot of Proxmox SAN storage {storage_id}");
    progress.step(format!(
        "creating ONTAP snapshot `{snapshot_name}` on volume `{}`",
        volume.name
    ));
    let result = ontap
        .create_snapshot(&volume, &snapshot_name, &snapshot_comment)
        .await;

    match &result {
        Ok(()) => progress.step("ONTAP snapshot create request completed"),
        Err(error) => progress.warn(format!(
            "ONTAP snapshot create failed; thawing frozen VMs before returning: {error}"
        )),
    }

    if frozen_vms.is_empty() {
        progress.step("no frozen VMs to thaw");
    } else {
        progress.step(format!("thawing {} frozen VM(s)", frozen_vms.len()));
    }

    for vm in frozen_vms {
        progress.step(format!(
            "thawing filesystem for VM {} ({}) with guest agent",
            vm.vmid, vm.name
        ));
        if let Err(error) = proxmox
            .run_guest_agent_command(&vm.node, vm.vmid, "fsfreeze-thaw")
            .await
        {
            progress.warn(format!(
                "fsfreeze-thaw failed for vm {} ({}): {}",
                vm.vmid, vm.name, error
            ));
        }
    }

    match result {
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
    output: OutputFormat,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let snapshots = storage_snapshot_rows(config, ontap, storage_id).await?;
    crate::display::print_snapshots(output, &snapshots)
}

pub async fn storage_snapshot_rows<O>(
    config: &LoadedConfig,
    ontap: &O,
    storage_id: &str,
) -> Result<Vec<SnapshotRow>>
where
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
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

pub async fn show_storage<P, O>(
    config: &LoadedConfig,
    _: &P,
    ontap: &O,
    storage_id: &str,
    output: OutputFormat,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    let san_config = require_san_config(config, storage_id)?;
    let volume = ontap.get_volume_by_name(&san_config.volume_name).await?;
    let lun = ontap.get_lun_by_name(&san_config.lun_path).await?;
    let mappings = ontap.list_lun_maps().await?;
    let volume_detail = ontap.get_volume_detail(&volume.uuid).await?;
    let lun_detail = ontap.get_lun_detail(&lun.uuid).await?;

    let mut igroups = Vec::new();
    for mapping in mappings
        .iter()
        .filter(|mapping| mapping.lun_name.as_deref() == Some(&san_config.lun_path))
    {
        if let Some(igroup_name) = &mapping.igroup_name {
            igroups.push(IgroupDetail {
                name: igroup_name.clone(),
                detail: ontap.get_igroup_detail(igroup_name).await?,
            });
        }
    }

    let lun_mappings = mappings
        .into_iter()
        .filter(|mapping| {
            mapping
                .lun_name
                .as_deref()
                .is_some_and(|name| name.contains(&san_config.volume_name))
        })
        .map(|mapping| mapping.raw)
        .collect::<Vec<_>>();

    let display = SanStorageDetail {
        volume: volume_detail,
        lun: lun_detail,
        igroups,
        lun_mappings,
    };

    let mut sections = vec![
        DetailSection::new("Volume Info", display.volume.clone()),
        DetailSection::new("LUN Info", display.lun.clone()),
    ];

    let igroup_section = if display.igroups.is_empty() {
        json!([])
    } else {
        Value::Array(
            display
                .igroups
                .iter()
                .map(|igroup| {
                    json!({
                        "name": igroup.name.clone(),
                        "detail": igroup.detail.clone(),
                    })
                })
                .collect(),
        )
    };
    sections.push(DetailSection::new(
        "iGroups (discovered from LUN mappings)",
        igroup_section,
    ));
    sections.push(DetailSection::new(
        "LUN Mappings",
        Value::Array(display.lun_mappings.clone()),
    ));

    crate::display::print_sections(output, &sections, &display)
}

#[derive(Debug, Serialize)]
struct SanStorageDetail {
    volume: Value,
    lun: Value,
    igroups: Vec<IgroupDetail>,
    lun_mappings: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct IgroupDetail {
    name: String,
    detail: Value,
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
