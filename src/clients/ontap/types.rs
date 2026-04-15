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
    #[serde(default)]
    pub(crate) path: Option<String>,
}

impl From<VolumeRecord> for OntapVolume {
    fn from(value: VolumeRecord) -> Self {
        Self {
            uuid: value.uuid,
            name: value.name,
            svm_name: value.svm.name,
            nas_path: value.nas.and_then(|nas| nas.path),
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RecordsEnvelope, VolumeRecord};
    use crate::models::OntapVolume;

    #[test]
    fn deserializes_volume_with_nas_object_without_path() {
        let envelope: RecordsEnvelope<VolumeRecord> = serde_json::from_value(json!({
            "records": [{
                "uuid": "vol-1",
                "name": "san_vol1",
                "svm": { "name": "svm1" },
                "nas": {}
            }]
        }))
        .expect("volume should deserialize");

        let volume: OntapVolume = envelope.records.into_iter().next().unwrap().into();
        assert_eq!(volume.nas_path, None);
    }
}
