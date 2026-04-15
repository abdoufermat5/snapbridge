use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{
    LoadedConfig, NasStorageConfig, ProxmoxConfig, SharedStorageConfig, StorageConfig,
};
use crate::error::{AppError, Result};
use crate::models::{
    FileCloneRequest, FlexCloneRequest, Node, OntapLun, OntapLunMap, OntapSnapshot, OntapVolume,
    ProxmoxStorage, TaskStatus, VmConfig, VmStatus, VmSummary,
};

use super::discovery::{get_vms_by_storage, wait_for_task};
use super::storage::{create_storage_snapshot, mount_storage_snapshot};
use super::vm::create_vm_snapshot_with_factory;

#[derive(Clone, Default)]
struct MockProxmox {
    nodes: Vec<Node>,
    status_by_vm: HashMap<(String, u32), VmStatus>,
    config_by_vm: HashMap<(String, u32), VmConfig>,
    vms_by_node: HashMap<String, Vec<VmSummary>>,
    tasks: Arc<Mutex<HashMap<(String, String), VecDeque<TaskStatus>>>>,
    storage_defs: HashMap<String, ProxmoxStorage>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ProxmoxApi for MockProxmox {
    async fn list_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.nodes.clone())
    }

    async fn list_vms(&self, node: &str) -> Result<Vec<VmSummary>> {
        Ok(self.vms_by_node.get(node).cloned().unwrap_or_default())
    }

    async fn get_vm_status(&self, node: &str, vmid: u32) -> Result<VmStatus> {
        self.status_by_vm
            .get(&(node.to_owned(), vmid))
            .cloned()
            .ok_or_else(|| AppError::Missing("vm status".to_owned()))
    }

    async fn get_vm_config(&self, node: &str, vmid: u32) -> Result<VmConfig> {
        self.config_by_vm
            .get(&(node.to_owned(), vmid))
            .cloned()
            .ok_or_else(|| AppError::Missing("vm config".to_owned()))
    }

    async fn shutdown_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("shutdown:{node}:{vmid}"));
        Ok("shutdown-task".to_owned())
    }

    async fn suspend_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("suspend:{node}:{vmid}"));
        Ok("suspend-task".to_owned())
    }

    async fn start_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("start:{node}:{vmid}"));
        Ok("start-task".to_owned())
    }

    async fn get_task_status(&self, node: &str, upid: &str) -> Result<TaskStatus> {
        let mut guard = self.tasks.lock().unwrap();
        let queue = guard
            .get_mut(&(node.to_owned(), upid.to_owned()))
            .ok_or_else(|| AppError::Missing("task".to_owned()))?;
        Ok(queue.pop_front().unwrap_or(TaskStatus {
            status: "stopped".to_owned(),
            exitstatus: Some("OK".to_owned()),
        }))
    }

    async fn create_vm_snapshot(
        &self,
        node: &str,
        vmid: u32,
        snapname: &str,
        _: &str,
    ) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("create-snapshot:{node}:{vmid}:{snapname}"));
        Ok(format!("create-{vmid}"))
    }

    async fn delete_vm_snapshot(&self, node: &str, vmid: u32, snapname: &str) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("delete-snapshot:{node}:{vmid}:{snapname}"));
        Ok(format!("delete-{vmid}"))
    }

    async fn run_guest_agent_command(&self, _: &str, _: u32, _: &str) -> Result<Value> {
        Ok(json!(1))
    }

    async fn get_storage(&self, storage: &str) -> Result<ProxmoxStorage> {
        self.storage_defs
            .get(storage)
            .cloned()
            .ok_or_else(|| AppError::Missing("storage".to_owned()))
    }

    async fn create_storage(&self, storage: &str, definition: &ProxmoxStorage) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("create-storage:{storage}:{}", definition.export));
        Ok(())
    }

    async fn delete_storage(&self, storage: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("delete-storage:{storage}"));
        Ok(())
    }
}

#[derive(Clone, Default)]
struct MockOntap {
    volume: Option<OntapVolume>,
    snapshots: Vec<OntapSnapshot>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl OntapApi for MockOntap {
    async fn get_volume_by_name(&self, _: &str) -> Result<OntapVolume> {
        self.volume
            .clone()
            .ok_or_else(|| AppError::Missing("volume".to_owned()))
    }

    async fn get_volume_detail(&self, _: &str) -> Result<Value> {
        Ok(json!({"name": "nasvol"}))
    }

    async fn list_snapshots(&self, _: &str) -> Result<Vec<OntapSnapshot>> {
        Ok(self.snapshots.clone())
    }

    async fn create_snapshot(&self, volume: &OntapVolume, name: &str, _: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("create-snapshot:{}:{name}", volume.name));
        Ok(())
    }

    async fn restore_snapshot(&self, volume: &OntapVolume, snapshot: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("restore:{}:{snapshot}", volume.name));
        Ok(())
    }

    async fn delete_snapshot(&self, volume_uuid: &str, snapshot_name: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("delete-snapshot:{volume_uuid}:{snapshot_name}"));
        Ok(())
    }

    async fn clone_file(&self, request: &FileCloneRequest) -> Result<()> {
        self.calls.lock().unwrap().push(format!(
            "clone-file:{}->{}",
            request.source_path, request.destination_path
        ));
        Ok(())
    }

    async fn create_flexclone(&self, request: &FlexCloneRequest) -> Result<()> {
        self.calls.lock().unwrap().push(format!(
            "flexclone:{}:{}",
            request.parent_volume_name, request.clone_name
        ));
        Ok(())
    }

    async fn delete_volume(&self, uuid: &str, force: bool) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("delete-volume:{uuid}:{force}"));
        Ok(())
    }

    async fn get_lun_by_name(&self, _: &str) -> Result<OntapLun> {
        Err(AppError::Missing("lun".to_owned()))
    }

    async fn get_lun_detail(&self, _: &str) -> Result<Value> {
        Err(AppError::Missing("lun".to_owned()))
    }

    async fn list_lun_maps(&self) -> Result<Vec<OntapLunMap>> {
        Ok(Vec::new())
    }

    async fn get_igroup_detail(&self, _: &str) -> Result<Value> {
        Err(AppError::Missing("igroup".to_owned()))
    }
}

fn sample_config() -> LoadedConfig {
    LoadedConfig {
        proxmox: ProxmoxConfig {
            host: "pve.local".to_owned(),
            user: "root@pam".to_owned(),
            token_name: "snap".to_owned(),
            token_value: "secret".to_owned(),
            verify_ssl: false,
            timezone: "Europe/Paris".to_owned(),
        },
        storage: [(
            "NAS01".to_owned(),
            StorageConfig::Nas(NasStorageConfig {
                common: SharedStorageConfig {
                    ontap_host: "ontap.local".to_owned(),
                    ontap_user: "admin".to_owned(),
                    ontap_password: "pw".to_owned(),
                    verify_ssl: false,
                },
            }),
        )]
        .into_iter()
        .collect(),
    }
}

#[tokio::test]
async fn waits_for_successful_task() {
    let proxmox = MockProxmox {
        tasks: Arc::new(Mutex::new(
            [(
                ("node1".to_owned(), "task1".to_owned()),
                VecDeque::from([
                    TaskStatus {
                        status: "running".to_owned(),
                        exitstatus: None,
                    },
                    TaskStatus {
                        status: "stopped".to_owned(),
                        exitstatus: Some("OK".to_owned()),
                    },
                ]),
            )]
            .into_iter()
            .collect(),
        )),
        ..Default::default()
    };

    let status = wait_for_task(&proxmox, "node1", "task1")
        .await
        .expect("task should complete");
    assert_eq!(status, "OK");
}

#[tokio::test]
async fn gets_vms_by_storage_from_scsi_disks() {
    let proxmox = MockProxmox {
        nodes: vec![Node {
            node: "node1".to_owned(),
        }],
        vms_by_node: [(
            "node1".to_owned(),
            vec![VmSummary {
                vmid: 101,
                name: Some("vm101".to_owned()),
                status: Some("running".to_owned()),
            }],
        )]
        .into_iter()
        .collect(),
        config_by_vm: [(
            ("node1".to_owned(), 101),
            BTreeMap::from([(
                "scsi0".to_owned(),
                json!("NAS01:101/vm-101-disk-0.raw,discard=on"),
            )]),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };

    let vms = get_vms_by_storage(&proxmox, "NAS01")
        .await
        .expect("lookup should work");
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].vmid, 101);
}

#[tokio::test]
async fn creates_nas_vm_clones_and_restarts_vm() {
    let proxmox = MockProxmox {
        nodes: vec![Node {
            node: "node1".to_owned(),
        }],
        status_by_vm: [(
            ("node1".to_owned(), 101),
            VmStatus {
                status: "running".to_owned(),
                name: Some("vm101".to_owned()),
            },
        )]
        .into_iter()
        .collect(),
        config_by_vm: [(
            ("node1".to_owned(), 101),
            BTreeMap::from([(
                "scsi0".to_owned(),
                json!("NAS01:101/vm-101-disk-0.raw,discard=on"),
            )]),
        )]
        .into_iter()
        .collect(),
        storage_defs: [(
            "NAS01".to_owned(),
            ProxmoxStorage {
                storage_type: "nfs".to_owned(),
                server: "nas.local".to_owned(),
                content: "images".to_owned(),
                export: "/nasvol".to_owned(),
            },
        )]
        .into_iter()
        .collect(),
        tasks: Arc::new(Mutex::new(
            [
                (
                    ("node1".to_owned(), "suspend-task".to_owned()),
                    VecDeque::from([TaskStatus {
                        status: "stopped".to_owned(),
                        exitstatus: Some("OK".to_owned()),
                    }]),
                ),
                (
                    ("node1".to_owned(), "start-task".to_owned()),
                    VecDeque::from([TaskStatus {
                        status: "stopped".to_owned(),
                        exitstatus: Some("OK".to_owned()),
                    }]),
                ),
            ]
            .into_iter()
            .collect(),
        )),
        ..Default::default()
    };
    let ontap = MockOntap {
        volume: Some(OntapVolume {
            uuid: "vol-1".to_owned(),
            name: "nasvol".to_owned(),
            svm_name: "svm1".to_owned(),
            nas_path: Some("/nasvol".to_owned()),
            is_flexclone: false,
        }),
        ..Default::default()
    };

    create_vm_snapshot_with_factory(&sample_config(), &proxmox, 101, true, false, |_| {
        Ok(ontap.clone())
    })
    .await
    .expect("vm snapshot should succeed");

    let calls = ontap.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("clone-file:images/101/vm-101-disk-0.raw"));

    let prox_calls = proxmox.calls.lock().unwrap().clone();
    assert_eq!(prox_calls, vec!["suspend:node1:101", "start:node1:101"]);
}

#[tokio::test]
async fn creates_nas_storage_snapshot_and_cleans_fsfreeze_snapshots() {
    let proxmox = MockProxmox {
        nodes: vec![Node {
            node: "node1".to_owned(),
        }],
        vms_by_node: [(
            "node1".to_owned(),
            vec![VmSummary {
                vmid: 101,
                name: Some("vm101".to_owned()),
                status: Some("running".to_owned()),
            }],
        )]
        .into_iter()
        .collect(),
        config_by_vm: [(
            ("node1".to_owned(), 101),
            BTreeMap::from([(
                "scsi0".to_owned(),
                json!("NAS01:101/vm-101-disk-0.raw,discard=on"),
            )]),
        )]
        .into_iter()
        .collect(),
        storage_defs: [(
            "NAS01".to_owned(),
            ProxmoxStorage {
                storage_type: "nfs".to_owned(),
                server: "nas.local".to_owned(),
                content: "images".to_owned(),
                export: "/nasvol".to_owned(),
            },
        )]
        .into_iter()
        .collect(),
        tasks: Arc::new(Mutex::new(
            [
                (
                    ("node1".to_owned(), "create-101".to_owned()),
                    VecDeque::from([TaskStatus {
                        status: "stopped".to_owned(),
                        exitstatus: Some("OK".to_owned()),
                    }]),
                ),
                (
                    ("node1".to_owned(), "delete-101".to_owned()),
                    VecDeque::from([TaskStatus {
                        status: "stopped".to_owned(),
                        exitstatus: Some("OK".to_owned()),
                    }]),
                ),
            ]
            .into_iter()
            .collect(),
        )),
        ..Default::default()
    };
    let ontap = MockOntap {
        volume: Some(OntapVolume {
            uuid: "vol-1".to_owned(),
            name: "nasvol".to_owned(),
            svm_name: "svm1".to_owned(),
            nas_path: Some("/nasvol".to_owned()),
            is_flexclone: false,
        }),
        ..Default::default()
    };

    create_storage_snapshot(&sample_config(), &proxmox, &ontap, "NAS01", true)
        .await
        .expect("storage snapshot should succeed");

    let prox_calls = proxmox.calls.lock().unwrap().clone();
    assert_eq!(prox_calls.len(), 2);
    assert!(prox_calls[0].starts_with("create-snapshot:node1:101:pve_ontap_"));
    assert!(prox_calls[1].starts_with("delete-snapshot:node1:101:pve_ontap_"));

    let ontap_calls = ontap.calls.lock().unwrap().clone();
    assert_eq!(ontap_calls.len(), 1);
    assert!(ontap_calls[0].starts_with("create-snapshot:nasvol:proxmox_snapshot_"));
}

#[tokio::test]
async fn mounts_nas_snapshot_as_clone_storage() {
    let proxmox = MockProxmox {
        storage_defs: [(
            "NAS01".to_owned(),
            ProxmoxStorage {
                storage_type: "nfs".to_owned(),
                server: "nas.local".to_owned(),
                content: "images".to_owned(),
                export: "/nasvol".to_owned(),
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let ontap = MockOntap {
        volume: Some(OntapVolume {
            uuid: "vol-1".to_owned(),
            name: "nasvol".to_owned(),
            svm_name: "svm1".to_owned(),
            nas_path: Some("/nasvol".to_owned()),
            is_flexclone: false,
        }),
        ..Default::default()
    };

    mount_storage_snapshot(&sample_config(), &proxmox, &ontap, "NAS01", "snap-1")
        .await
        .expect("mount should succeed");

    let ontap_calls = ontap.calls.lock().unwrap().clone();
    assert_eq!(ontap_calls, vec!["flexclone:nasvol:nasvol_clone"]);
    let prox_calls = proxmox.calls.lock().unwrap().clone();
    assert_eq!(prox_calls, vec!["create-storage:NAS01-CLONE:/nasvol_clone"]);
}
