use log::debug;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::SharedStorageConfig;
use crate::error::{AppError, Result};

use super::types::RecordsEnvelope;

#[derive(Clone)]
pub(crate) struct OntapHttpClient {
    client: Client,
    base_url: String,
    user: String,
    password: String,
}

impl OntapHttpClient {
    pub(crate) fn new(config: &SharedStorageConfig) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(!config.verify_ssl)
            .build()?;

        Ok(Self {
            client,
            base_url: format!("{}/api", config.base_url().trim_end_matches('/')),
            user: config.ontap_user.clone(),
            password: config.ontap_password.clone(),
        })
    }

    pub(crate) async fn get_records<T>(&self, path: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let response = self
            .client
            .get(self.url(path))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;
        let body = self.read_body(response).await?;
        let envelope: RecordsEnvelope<T> = serde_json::from_str(&body)?;
        Ok(envelope.records)
    }

    pub(crate) async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(self.url(path))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;
        let body = self.read_body(response).await?;
        Ok(serde_json::from_str(&body)?)
    }

    pub(crate) async fn post_json(&self, path: &str, body: Value) -> Result<()> {
        let response = self
            .client
            .post(self.url(path))
            .basic_auth(&self.user, Some(&self.password))
            .json(&body)
            .send()
            .await?;
        self.ensure_success(response).await
    }

    pub(crate) async fn delete(&self, path: &str) -> Result<()> {
        let response = self
            .client
            .delete(self.url(path))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await?;
        self.ensure_success(response).await
    }

    async fn read_body(&self, response: reqwest::Response) -> Result<String> {
        let status = response.status();
        let body = response.text().await?;
        debug!("ontap response ({status}): {body}");
        if status == StatusCode::NOT_FOUND {
            return Err(AppError::Missing(body));
        }
        if !status.is_success() {
            return Err(AppError::Unexpected(body));
        }
        Ok(body)
    }

    async fn ensure_success(&self, response: reqwest::Response) -> Result<()> {
        self.read_body(response).await?;
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}
