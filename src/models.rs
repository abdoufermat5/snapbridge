use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Node {
    pub node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct VmSummary {
    pub vmid: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct VmStatus {
    pub status: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TaskStatus {
    pub status: String,
    #[serde(default)]
    pub exitstatus: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProxmoxStorage {
    #[serde(rename = "type")]
    pub storage_type: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub export: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmRef {
    pub node: String,
    pub vmid: u32,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NasDisk {
    pub storage: String,
    pub disk_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntapVolume {
    pub uuid: String,
    pub name: String,
    pub svm_name: String,
    pub nas_path: Option<String>,
    pub is_flexclone: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntapSnapshot {
    pub uuid: String,
    pub name: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntapLun {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntapLunMap {
    pub lun_name: Option<String>,
    pub igroup_name: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexCloneRequest {
    pub parent_volume_name: String,
    pub parent_snapshot_name: String,
    pub clone_name: String,
    pub svm_name: String,
    pub nas_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCloneRequest {
    pub volume_name: String,
    pub volume_uuid: String,
    pub source_path: String,
    pub destination_path: String,
    pub overwrite_destination: bool,
}

pub type VmConfig = BTreeMap<String, Value>;
