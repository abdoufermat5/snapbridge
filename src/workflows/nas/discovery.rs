use tokio::time::{Duration, sleep};

use crate::clients::proxmox::ProxmoxApi;
use crate::error::{AppError, Result};
use crate::logger::ProgressLogger;
use crate::models::VmRef;
use crate::util::vm_uses_storage_scsi_only;

pub async fn resolve_nas_volume_name<P>(proxmox: &P, storage_id: &str) -> Result<String>
where
    P: ProxmoxApi,
{
    let storage = proxmox.get_storage(storage_id).await?;
    let export = storage.export.trim_matches('/');
    if export.is_empty() {
        return Err(AppError::Unexpected(format!(
            "storage `{storage_id}` has no export path"
        )));
    }
    Ok(export.to_owned())
}

pub async fn get_vms_by_storage<P>(proxmox: &P, storage_id: &str) -> Result<Vec<VmRef>>
where
    P: ProxmoxApi,
{
    let mut result = Vec::new();
    let progress = ProgressLogger::new("proxmox", "discover-vms", storage_id);

    for node in proxmox.list_nodes().await? {
        let vms = match proxmox.list_vms(&node.node).await {
            Ok(vms) => vms,
            Err(error) => {
                progress.warn(format!("failed to list VMs on {}: {}", node.node, error));
                continue;
            }
        };

        for vm in vms {
            let config = match proxmox.get_vm_config(&node.node, vm.vmid).await {
                Ok(config) => config,
                Err(error) => {
                    progress.warn(format!(
                        "failed to fetch config for vm {} on {}: {}",
                        vm.vmid, node.node, error
                    ));
                    continue;
                }
            };

            if vm_uses_storage_scsi_only(&config, storage_id) {
                result.push(VmRef {
                    node: node.node.clone(),
                    vmid: vm.vmid,
                    name: vm.name.unwrap_or_else(|| vm.vmid.to_string()),
                    status: vm.status.unwrap_or_else(|| "unknown".to_owned()),
                });
            }
        }
    }

    Ok(result)
}

pub(crate) async fn find_vm<P>(proxmox: &P, vmid: u32) -> Result<VmRef>
where
    P: ProxmoxApi,
{
    for node in proxmox.list_nodes().await? {
        let status = match proxmox.get_vm_status(&node.node, vmid).await {
            Ok(status) => status,
            Err(_) => continue,
        };

        let name = status.name.clone().unwrap_or_else(|| vmid.to_string());
        return Ok(VmRef {
            node: node.node,
            vmid,
            name,
            status: status.status,
        });
    }

    Err(AppError::Missing(format!("vm `{vmid}` not found")))
}

pub async fn wait_for_task<P>(proxmox: &P, node: &str, task: &str) -> Result<String>
where
    P: ProxmoxApi,
{
    loop {
        let task_status = proxmox.get_task_status(node, task).await?;
        if task_status.status == "stopped" || task_status.status == "error" {
            let exitstatus = task_status.exitstatus.unwrap_or_else(|| "OK".to_owned());
            if exitstatus != "OK" {
                return Err(AppError::TaskFailed(exitstatus));
            }
            return Ok(exitstatus);
        }
        sleep(Duration::from_millis(10)).await;
    }
}
