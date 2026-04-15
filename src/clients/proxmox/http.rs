use log::debug;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;

use crate::config::ProxmoxConfig;
use crate::error::{AppError, Result};

use super::types::ProxmoxEnvelope;

#[derive(Clone)]
pub(crate) struct ProxmoxHttpClient {
    client: Client,
    base_url: String,
    auth_header: String,
}

impl ProxmoxHttpClient {
    pub(crate) fn new(config: &ProxmoxConfig) -> Result<Self> {
        let base_host = if config.host.starts_with("http://") || config.host.starts_with("https://")
        {
            config.host.clone()
        } else {
            format!("https://{}:8006", config.host)
        };

        let client = Client::builder()
            .danger_accept_invalid_certs(!config.verify_ssl)
            .build()?;

        Ok(Self {
            client,
            base_url: format!("{}/api2/json", base_host.trim_end_matches('/')),
            auth_header: format!(
                "PVEAPIToken={}!{}={}",
                config.user, config.token_name, config.token_value
            ),
        })
    }

    pub(crate) async fn get<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .client
            .get(self.url(path))
            .header("Authorization", &self.auth_header)
            .send()
            .await?;
        self.parse_response(response).await
    }

    pub(crate) async fn post_form<T>(&self, path: &str, params: &[(&str, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .client
            .post(self.url(path))
            .header("Authorization", &self.auth_header)
            .form(params)
            .send()
            .await?;
        self.parse_response(response).await
    }

    pub(crate) async fn delete<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .client
            .delete(self.url(path))
            .header("Authorization", &self.auth_header)
            .send()
            .await?;
        self.parse_response(response).await
    }

    async fn parse_response<T>(&self, response: reqwest::Response) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let status = response.status();
        let body = response.text().await?;
        debug!("proxmox response ({status}): {body}");

        if status == StatusCode::NOT_FOUND {
            return Err(AppError::Missing(body));
        }
        if !status.is_success() {
            return Err(AppError::Unexpected(body));
        }

        let envelope: ProxmoxEnvelope<T> = serde_json::from_str(&body)?;
        Ok(envelope.data)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}
