use serde::Deserialize;

use crate::models::{OntapLun, OntapSnapshot, OntapVolume};

#[derive(Debug, Deserialize)]
pub(crate) struct RecordsEnvelope<T> {
    pub(crate) records: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VolumeRecord {
    pub(crate) uuid: String,
    pub(crate) name: String,
    pub(crate) svm: NameRecord,
    #[serde(default)]
    pub(crate) nas: Option<PathRecord>,
    #[serde(default)]
    pub(crate) clone: Option<FlexCloneRecord>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FlexCloneRecord {
    #[serde(default)]
    pub(crate) is_flexclone: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotRecord {
    pub(crate) uuid: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) comment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LunRecord {
    pub(crate) uuid: String,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NameRecord {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PathRecord {
    pub(crate) path: String,
}

impl From<VolumeRecord> for OntapVolume {
    fn from(value: VolumeRecord) -> Self {
        Self {
            uuid: value.uuid,
            name: value.name,
            svm_name: value.svm.name,
            nas_path: value.nas.map(|nas| nas.path),
            is_flexclone: value.clone.is_some_and(|clone| clone.is_flexclone),
        }
    }
}

impl From<SnapshotRecord> for OntapSnapshot {
    fn from(value: SnapshotRecord) -> Self {
        Self {
            uuid: value.uuid,
            name: value.name,
            comment: value.comment,
        }
    }
}

impl From<LunRecord> for OntapLun {
    fn from(value: LunRecord) -> Self {
        Self {
            uuid: value.uuid,
            name: value.name,
        }
    }
}
