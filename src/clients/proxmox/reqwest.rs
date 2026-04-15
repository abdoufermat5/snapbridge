use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;
use urlencoding::encode;

use crate::config::ProxmoxConfig;
use crate::error::Result;
use crate::models::{Node, ProxmoxStorage, TaskStatus, VmConfig, VmStatus, VmSummary};

use super::api::ProxmoxApi;
use super::http::ProxmoxHttpClient;

#[derive(Clone)]
pub struct ReqwestProxmoxClient {
    http: ProxmoxHttpClient,
}

impl ReqwestProxmoxClient {
    pub fn new(config: &ProxmoxConfig) -> Result<Self> {
        Ok(Self {
            http: ProxmoxHttpClient::new(config)?,
        })
    }
}

#[async_trait]
impl ProxmoxApi for ReqwestProxmoxClient {
    async fn list_nodes(&self) -> Result<Vec<Node>> {
        self.http.get("/nodes").await
    }

    async fn list_vms(&self, node: &str) -> Result<Vec<VmSummary>> {
        self.http
            .get(&format!("/nodes/{}/qemu", encode(node)))
            .await
    }

    async fn get_vm_status(&self, node: &str, vmid: u32) -> Result<VmStatus> {
        self.http
            .get(&format!(
                "/nodes/{}/qemu/{vmid}/status/current",
                encode(node)
            ))
            .await
    }

    async fn get_vm_config(&self, node: &str, vmid: u32) -> Result<VmConfig> {
        let data: BTreeMap<String, Value> = self
            .http
            .get(&format!("/nodes/{}/qemu/{vmid}/config", encode(node)))
            .await?;
        Ok(data)
    }

    async fn shutdown_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.http
            .post_form(
                &format!("/nodes/{}/qemu/{vmid}/status/shutdown", encode(node)),
                &[],
            )
            .await
    }

    async fn suspend_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.http
            .post_form(
                &format!("/nodes/{}/qemu/{vmid}/status/suspend", encode(node)),
                &[("todisk", "1".to_owned())],
            )
            .await
    }

    async fn start_vm(&self, node: &str, vmid: u32) -> Result<String> {
        self.http
            .post_form(
                &format!("/nodes/{}/qemu/{vmid}/status/start", encode(node)),
                &[],
            )
            .await
    }

    async fn get_task_status(&self, node: &str, upid: &str) -> Result<TaskStatus> {
        self.http
            .get(&format!(
                "/nodes/{}/tasks/{}/status",
                encode(node),
                encode(upid)
            ))
            .await
    }

    async fn create_vm_snapshot(
        &self,
        node: &str,
        vmid: u32,
        snapname: &str,
        description: &str,
    ) -> Result<String> {
        self.http
            .post_form(
                &format!("/nodes/{}/qemu/{vmid}/snapshot", encode(node)),
                &[
                    ("snapname", snapname.to_owned()),
                    ("description", description.to_owned()),
                ],
            )
            .await
    }

    async fn delete_vm_snapshot(&self, node: &str, vmid: u32, snapname: &str) -> Result<String> {
        self.http
            .delete(&format!(
                "/nodes/{}/qemu/{vmid}/snapshot/{}",
                encode(node),
                encode(snapname)
            ))
            .await
    }

    async fn run_guest_agent_command(&self, node: &str, vmid: u32, command: &str) -> Result<Value> {
        self.http
            .post_form(
                &format!(
                    "/nodes/{}/qemu/{vmid}/agent/{}",
                    encode(node),
                    encode(command)
                ),
                &[],
            )
            .await
    }

    async fn get_storage(&self, storage: &str) -> Result<ProxmoxStorage> {
        self.http
            .get(&format!("/storage/{}", encode(storage)))
            .await
    }

    async fn create_storage(&self, storage: &str, definition: &ProxmoxStorage) -> Result<()> {
        let _: Value = self
            .http
            .post_form(
                "/storage",
                &[
                    ("storage", storage.to_owned()),
                    ("server", definition.server.clone()),
                    ("type", definition.storage_type.clone()),
                    ("content", definition.content.clone()),
                    ("export", definition.export.clone()),
                ],
            )
            .await?;
        Ok(())
    }

    async fn delete_storage(&self, storage: &str) -> Result<()> {
        let _: Value = self
            .http
            .delete(&format!("/storage/{}", encode(storage)))
            .await?;
        Ok(())
    }
}
