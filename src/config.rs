use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct LoadedConfig {
    pub proxmox: ProxmoxConfig,
    pub storage: BTreeMap<String, StorageConfig>,
    #[serde(default)]
    pub schedule: BTreeMap<String, ScheduleConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ConfigDocument {
    Modern(LoadedConfig),
    Legacy(LegacyLoadedConfig),
}

#[derive(Debug, Deserialize)]
struct LegacyLoadedConfig {
    proxmox: ProxmoxConfig,
    #[serde(flatten)]
    storage: BTreeMap<String, StorageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxmoxConfig {
    pub host: String,
    pub user: String,
    pub token_name: String,
    pub token_value: String,
    #[serde(default)]
    pub verify_ssl: bool,
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Nas,
    San,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum StorageConfig {
    Nas(NasStorageConfig),
    San(SanStorageConfig),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SharedStorageConfig {
    pub ontap_host: String,
    pub ontap_user: String,
    pub ontap_password: String,
    #[serde(default)]
    pub verify_ssl: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NasStorageConfig {
    #[serde(flatten)]
    pub common: SharedStorageConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SanStorageConfig {
    #[serde(flatten)]
    pub common: SharedStorageConfig,
    pub volume_name: String,
    pub lun_path: String,
    #[serde(default = "default_ssh_user")]
    pub ssh_user: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleConfig {
    pub storages: Vec<String>,
    #[serde(default)]
    pub fsfreeze: bool,
    #[serde(default)]
    pub keep_last: Option<usize>,
    #[serde(default)]
    pub max_age: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub keep_last: Option<usize>,
    pub max_age: Option<Duration>,
}

impl LoadedConfig {
    pub fn from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let document: ConfigDocument = toml::from_str(content)?;
        Ok(match document {
            ConfigDocument::Modern(config) => config,
            ConfigDocument::Legacy(config) => Self {
                proxmox: config.proxmox,
                storage: config.storage,
                schedule: BTreeMap::new(),
            },
        })
    }

    pub fn storage(&self, id: &str) -> Result<&StorageConfig> {
        let key = id.strip_suffix("-CLONE").unwrap_or(id);
        self.storage
            .get(key)
            .ok_or_else(|| AppError::Config(format!("storage `{id}` is not configured")))
    }

    pub fn require_backend(&self, id: &str, expected: StorageBackend) -> Result<&StorageConfig> {
        let storage = self.storage(id)?;
        let actual = match storage {
            StorageConfig::Nas(_) => StorageBackend::Nas,
            StorageConfig::San(_) => StorageBackend::San,
        };

        if actual != expected {
            return Err(AppError::Config(format!(
                "storage `{id}` uses backend `{actual}` but `{expected}` was required"
            )));
        }

        Ok(storage)
    }

    pub fn storage_ids_for_backend(&self, expected: StorageBackend) -> Vec<&str> {
        self.storage
            .iter()
            .filter_map(|(id, storage)| {
                let actual = match storage {
                    StorageConfig::Nas(_) => StorageBackend::Nas,
                    StorageConfig::San(_) => StorageBackend::San,
                };

                (actual == expected).then_some(id.as_str())
            })
            .collect()
    }

    pub fn schedule(&self, name: &str) -> Result<&ScheduleConfig> {
        self.schedule
            .get(name)
            .ok_or_else(|| AppError::Config(format!("schedule `{name}` is not configured")))
    }
}

impl ScheduleConfig {
    pub fn retention_policy(&self) -> Result<Option<RetentionPolicy>> {
        let max_age = self.max_age.as_deref().map(parse_duration).transpose()?;

        Ok(
            (self.keep_last.is_some() || max_age.is_some()).then_some(RetentionPolicy {
                keep_last: self.keep_last,
                max_age,
            }),
        )
    }
}

impl std::fmt::Display for StorageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nas => write!(f, "nas"),
            Self::San => write!(f, "san"),
        }
    }
}

impl SharedStorageConfig {
    pub fn base_url(&self) -> String {
        if self.ontap_host.starts_with("http://") || self.ontap_host.starts_with("https://") {
            self.ontap_host.clone()
        } else {
            format!("https://{}", self.ontap_host)
        }
    }
}

impl NasStorageConfig {
    pub fn shared(&self) -> &SharedStorageConfig {
        &self.common
    }
}

impl SanStorageConfig {
    pub fn shared(&self) -> &SharedStorageConfig {
        &self.common
    }
}

fn default_timezone() -> String {
    "Europe/Paris".to_owned()
}

fn default_ssh_user() -> String {
    "root".to_owned()
}

fn parse_duration(value: &str) -> Result<Duration> {
    let value = value.trim();
    if value.len() < 2 {
        return Err(AppError::Config(format!("invalid duration `{value}`")));
    }

    let (number, unit) = value.split_at(value.len() - 1);
    let amount = number
        .parse::<u64>()
        .map_err(|_| AppError::Config(format!("invalid duration `{value}`")))?;

    let seconds = match unit {
        "s" => amount,
        "m" => amount
            .checked_mul(60)
            .ok_or_else(|| AppError::Config(format!("duration `{value}` is too large")))?,
        "h" => amount
            .checked_mul(60 * 60)
            .ok_or_else(|| AppError::Config(format!("duration `{value}` is too large")))?,
        "d" => amount
            .checked_mul(24 * 60 * 60)
            .ok_or_else(|| AppError::Config(format!("duration `{value}` is too large")))?,
        "w" => amount
            .checked_mul(7 * 24 * 60 * 60)
            .ok_or_else(|| AppError::Config(format!("duration `{value}` is too large")))?,
        _ => {
            return Err(AppError::Config(format!(
                "invalid duration `{value}`; expected suffix s, m, h, d, or w"
            )));
        }
    };

    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{LoadedConfig, StorageBackend};

    #[test]
    fn parses_toml_and_routes_clone_storage() {
        let config = LoadedConfig::parse(
            r#"
            [proxmox]
            host = "pve.local"
            user = "root@pam"
            token_name = "snap"
            token_value = "secret"
            verify_ssl = false

            [storage.NAS01]
            backend = "nas"
            ontap_host = "ontap.local"
            ontap_user = "admin"
            ontap_password = "pw"
            verify_ssl = false

            [storage.SAN01]
            backend = "san"
            ontap_host = "ontap.local"
            ontap_user = "admin"
            ontap_password = "pw"
            verify_ssl = false
            volume_name = "san_vol1"
            lun_path = "/vol/san_vol1/lun0"
            ssh_user = "root"

            [schedule.daily]
            storages = ["NAS01", "SAN01"]
            fsfreeze = true
            keep_last = 7
            max_age = "30d"
            "#,
        )
        .expect("config should parse");

        assert!(config.storage("NAS01-CLONE").is_ok());
        assert!(config.require_backend("NAS01", StorageBackend::Nas).is_ok());
        assert!(config.require_backend("SAN01", StorageBackend::San).is_ok());
        assert!(
            config
                .require_backend("SAN01", StorageBackend::Nas)
                .is_err()
        );
        assert_eq!(
            config.storage_ids_for_backend(StorageBackend::Nas),
            vec!["NAS01"]
        );
        assert_eq!(
            config.storage_ids_for_backend(StorageBackend::San),
            vec!["SAN01"]
        );

        let schedule = config.schedule("daily").expect("schedule should exist");
        assert_eq!(schedule.storages, vec!["NAS01", "SAN01"]);
        assert!(schedule.fsfreeze);
        assert_eq!(schedule.keep_last, Some(7));
        assert_eq!(
            schedule
                .retention_policy()
                .expect("retention should parse")
                .expect("retention should exist")
                .max_age,
            Some(Duration::from_secs(30 * 24 * 60 * 60))
        );
    }

    #[test]
    fn parses_legacy_top_level_storage_sections() {
        let config = LoadedConfig::parse(
            r#"
            [proxmox]
            host = "pve.local"
            user = "root@pam"
            token_name = "snap"
            token_value = "secret"
            verify_ssl = false

            [NAS01]
            backend = "nas"
            ontap_host = "ontap.local"
            ontap_user = "admin"
            ontap_password = "pw"
            verify_ssl = false

            [SAN01]
            backend = "san"
            ontap_host = "ontap.local"
            ontap_user = "admin"
            ontap_password = "pw"
            verify_ssl = false
            volume_name = "san_vol1"
            lun_path = "/vol/san_vol1/lun0"
            "#,
        )
        .expect("legacy config should parse");

        assert!(config.require_backend("NAS01", StorageBackend::Nas).is_ok());
        assert!(config.require_backend("SAN01", StorageBackend::San).is_ok());
        assert_eq!(
            config.storage_ids_for_backend(StorageBackend::Nas),
            vec!["NAS01"]
        );
        assert_eq!(
            config.storage_ids_for_backend(StorageBackend::San),
            vec!["SAN01"]
        );
    }

    #[test]
    fn rejects_invalid_schedule_duration() {
        let config = LoadedConfig::parse(
            r#"
            [proxmox]
            host = "pve.local"
            user = "root@pam"
            token_name = "snap"
            token_value = "secret"

            [storage.NAS01]
            backend = "nas"
            ontap_host = "ontap.local"
            ontap_user = "admin"
            ontap_password = "pw"

            [schedule.daily]
            storages = ["NAS01"]
            max_age = "one month"
            "#,
        )
        .expect("config should parse");

        assert!(
            config
                .schedule("daily")
                .unwrap()
                .retention_policy()
                .is_err()
        );
    }
}
