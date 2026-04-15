use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{
    LoadedConfig, ProxmoxConfig, SanStorageConfig, SharedStorageConfig, StorageConfig,
};
use crate::error::{AppError, Result};
use crate::models::{
    FileCloneRequest, FlexCloneRequest, Node, OntapLun, OntapLunMap, OntapSnapshot, OntapVolume,
    ProxmoxStorage, TaskStatus, VmConfig, VmStatus, VmSummary,
};
use crate::shell::ShellRunner;

use super::storage::{create_storage_snapshot, restore_storage_snapshot};

#[derive(Clone, Default)]
struct MockProxmox {
    nodes: Vec<Node>,
    vms_by_node: HashMap<String, Vec<VmSummary>>,
    config_by_vm: HashMap<(String, u32), VmConfig>,
    agent_calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ProxmoxApi for MockProxmox {
    async fn list_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.nodes.clone())
    }

    async fn list_vms(&self, node: &str) -> Result<Vec<VmSummary>> {
        Ok(self.vms_by_node.get(node).cloned().unwrap_or_default())
    }

    async fn get_vm_status(&self, _: &str, _: u32) -> Result<VmStatus> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn get_vm_config(&self, node: &str, vmid: u32) -> Result<VmConfig> {
        self.config_by_vm
            .get(&(node.to_owned(), vmid))
            .cloned()
            .ok_or_else(|| AppError::Missing("config".to_owned()))
    }

    async fn shutdown_vm(&self, _: &str, _: u32) -> Result<String> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn suspend_vm(&self, _: &str, _: u32) -> Result<String> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn start_vm(&self, _: &str, _: u32) -> Result<String> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn get_task_status(&self, _: &str, _: &str) -> Result<TaskStatus> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn create_vm_snapshot(&self, _: &str, _: u32, _: &str, _: &str) -> Result<String> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn delete_vm_snapshot(&self, _: &str, _: u32, _: &str) -> Result<String> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn run_guest_agent_command(&self, node: &str, vmid: u32, command: &str) -> Result<Value> {
        self.agent_calls
            .lock()
            .unwrap()
            .push(format!("{node}:{vmid}:{command}"));
        Ok(json!(1))
    }

    async fn get_storage(&self, _: &str) -> Result<ProxmoxStorage> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn create_storage(&self, _: &str, _: &ProxmoxStorage) -> Result<()> {
        Err(AppError::Missing("unused".to_owned()))
    }

    async fn delete_storage(&self, _: &str) -> Result<()> {
        Err(AppError::Missing("unused".to_owned()))
    }
}

#[derive(Clone, Default)]
struct MockOntap {
    volume: Option<OntapVolume>,
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
        Ok(json!({}))
    }

    async fn list_snapshots(&self, _: &str) -> Result<Vec<OntapSnapshot>> {
        Ok(Vec::new())
    }

    async fn create_snapshot(&self, volume: &OntapVolume, name: &str, _: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("create:{}:{name}", volume.name));
        Ok(())
    }

    async fn restore_snapshot(&self, volume: &OntapVolume, snapshot: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("restore:{}:{snapshot}", volume.name));
        Ok(())
    }

    async fn delete_snapshot(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }

    async fn clone_file(&self, _: &FileCloneRequest) -> Result<()> {
        Ok(())
    }

    async fn create_flexclone(&self, _: &FlexCloneRequest) -> Result<()> {
        Ok(())
    }

    async fn delete_volume(&self, _: &str, _: bool) -> Result<()> {
        Ok(())
    }

    async fn get_lun_by_name(&self, _: &str) -> Result<OntapLun> {
        Ok(OntapLun {
            uuid: "lun-1".to_owned(),
            name: "/vol/san_vol1/lun0".to_owned(),
        })
    }

    async fn get_lun_detail(&self, _: &str) -> Result<Value> {
        Ok(json!({}))
    }

    async fn list_lun_maps(&self) -> Result<Vec<OntapLunMap>> {
        Ok(Vec::new())
    }

    async fn get_igroup_detail(&self, _: &str) -> Result<Value> {
        Ok(json!({}))
    }
}

#[derive(Clone, Default)]
struct MockShell {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ShellRunner for MockShell {
    async fn ssh(&self, host: &str, user: &str, command: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{user}@{host}:{command}"));
        Ok(())
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
            "SAN01".to_owned(),
            StorageConfig::San(SanStorageConfig {
                common: SharedStorageConfig {
                    ontap_host: "ontap.local".to_owned(),
                    ontap_user: "admin".to_owned(),
                    ontap_password: "pw".to_owned(),
                    verify_ssl: false,
                },
                volume_name: "san_vol1".to_owned(),
                lun_path: "/vol/san_vol1/lun0".to_owned(),
                ssh_user: "root".to_owned(),
            }),
        )]
        .into_iter()
        .collect(),
        schedule: BTreeMap::new(),
    }
}

#[tokio::test]
async fn creates_san_snapshot_with_freeze_and_thaw() {
    let proxmox = MockProxmox {
        nodes: vec![Node {
            node: "node1".to_owned(),
        }],
        vms_by_node: [(
            "node1".to_owned(),
            vec![VmSummary {
                vmid: 202,
                name: Some("vm202".to_owned()),
                status: Some("running".to_owned()),
            }],
        )]
        .into_iter()
        .collect(),
        config_by_vm: [(
            ("node1".to_owned(), 202),
            BTreeMap::from([("scsi0".to_owned(), json!("SAN01:vm-202-disk-0"))]),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let ontap = MockOntap {
        volume: Some(OntapVolume {
            uuid: "vol-1".to_owned(),
            name: "san_vol1".to_owned(),
            svm_name: "svm1".to_owned(),
            nas_path: None,
            is_flexclone: false,
        }),
        ..Default::default()
    };

    create_storage_snapshot(&sample_config(), &proxmox, &ontap, "SAN01", true)
        .await
        .expect("san snapshot should succeed");

    let agent_calls = proxmox.agent_calls.lock().unwrap().clone();
    assert_eq!(
        agent_calls,
        vec![
            "node1:202:fsfreeze-freeze".to_owned(),
            "node1:202:fsfreeze-thaw".to_owned()
        ]
    );
    let ontap_calls = ontap.calls.lock().unwrap().clone();
    assert_eq!(ontap_calls.len(), 1);
    assert!(ontap_calls[0].starts_with("create:san_vol1:proxmox_snapshot_"));
}

#[tokio::test]
async fn san_restore_rescans_every_node_over_ssh() {
    let proxmox = MockProxmox {
        nodes: vec![
            Node {
                node: "node1".to_owned(),
            },
            Node {
                node: "node2".to_owned(),
            },
        ],
        ..Default::default()
    };
    let ontap = MockOntap {
        volume: Some(OntapVolume {
            uuid: "vol-1".to_owned(),
            name: "san_vol1".to_owned(),
            svm_name: "svm1".to_owned(),
            nas_path: None,
            is_flexclone: false,
        }),
        ..Default::default()
    };
    let shell = MockShell::default();

    restore_storage_snapshot(
        &sample_config(),
        &proxmox,
        &ontap,
        &shell,
        "SAN01",
        "snap-1",
    )
    .await
    .expect("restore should succeed");

    let ssh_calls = shell.calls.lock().unwrap().clone();
    assert_eq!(
        ssh_calls,
        vec![
            "root@node1:iscsiadm -m session --rescan",
            "root@node1:pvscan --cache",
            "root@node2:iscsiadm -m session --rescan",
            "root@node2:pvscan --cache",
        ]
    );
}
