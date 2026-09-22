use belarc_shared::{HeartbeatPayload, InventoryPayload, RegisterPayload};
use serde::Deserialize;
use thiserror::Error;

use crate::config::AgentConfig;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server error: {0}")]
    Server(String),
}

pub struct ServerClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct DevicePortalSession {
    pub token: String,
    pub portal_path: String,
}

impl ServerClient {
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: config.server_url.trim_end_matches('/').to_string(),
            token: config.agent_token.clone(),
        }
    }

    pub async fn register(&self, payload: RegisterPayload) -> Result<(), ClientError> {
        let url = format!("{}/api/register", self.base_url);
        let resp = self.http.post(&url).json(&payload).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Server(resp.text().await.unwrap_or_default()))
        }
    }

    pub async fn heartbeat(&self, mut payload: HeartbeatPayload) -> Result<(), ClientError> {
        payload.agent_token = self.token.clone();
        let url = format!("{}/api/heartbeat", self.base_url);
        let resp = self.http.post(&url).json(&payload).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Server(resp.text().await.unwrap_or_default()))
        }
    }

    pub async fn inventory(&self, mut payload: InventoryPayload) -> Result<(), ClientError> {
        payload.agent_token = self.token.clone();
        let url = format!("{}/api/inventory", self.base_url);
        let resp = self.http.post(&url).json(&payload).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Server(resp.text().await.unwrap_or_default()))
        }
    }

    pub async fn flush_pending(&self, cache: &crate::cache::LocalCache) -> Result<(), ClientError> {
        let pending = cache
            .drain_pending()
            .map_err(|e| ClientError::Server(e.to_string()))?;
        for (id, payload_type, json) in pending {
            let url = match payload_type.as_str() {
                "heartbeat" => format!("{}/api/heartbeat", self.base_url),
                "inventory" => format!("{}/api/inventory", self.base_url),
                _ => continue,
            };
            let resp = self
                .http
                .post(&url)
                .body(json)
                .header("content-type", "application/json")
                .send()
                .await?;
            if resp.status().is_success() {
                let _ = cache.remove_pending(id);
            } else {
                let _ = cache.increment_attempts(id);
            }
        }
        Ok(())
    }

    pub async fn create_device_portal_session(
        &self,
        mode: &str,
    ) -> Result<DevicePortalSession, ClientError> {
        let url = format!("{}/api/portal/device-session", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "mode": mode }))
            .send()
            .await?;
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Server(
                response.text().await.unwrap_or_default(),
            ))
        }
    }

    pub fn portal_url(&self, session: &DevicePortalSession) -> String {
        // A versão na query evita que uma instalação PWA antiga entregue um
        // shell em cache que ainda mostrava o login manual.
        format!(
            "{}{}?v=portal-autenticado-v2#access_token={}",
            self.base_url, session.portal_path, session.token
        )
    }
}
