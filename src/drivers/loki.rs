use crate::types::{LogDriver, Message};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::json;
use tracing::debug;

pub struct LokiDriver {
    client: HttpClient,
    url: String,
    username: String,
    password: String,
}

impl LokiDriver {
    pub fn new(url: String, username: String, password: String) -> Self {
        Self {
            client: HttpClient::new(),
            url,
            username,
            password,
        }
    }
}

#[async_trait]
impl LogDriver for LokiDriver {
    async fn init(&mut self) -> Result<()> {
        debug!("init loki");
        Ok(())
    }

    async fn send_log(&mut self, message: &Message) -> Result<()> {
        debug!("sending log via loki");
        let labels = json!({
            "project": message.project_name,
            "deployment": message.deployment_id,
            "source": message.source,
            "environment": message.environment,
            "branch": message.branch,
        });

        let payload = json!({
            "streams": [{
                "stream": labels,
                "values": [[
                    (message.timestamp * 1000000).to_string(),
                    serde_json::to_string(message)?
                ]]
            }]
        });
        debug!("formed payload");

        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&payload);

        debug!("built request");
        if !self.username.is_empty() && !self.password.is_empty() {
            req = req.basic_auth(&self.username, Some(&self.password))
        }
        let response = req.send().await?;
        debug!("sent request");

        if !response.status().is_success() {
            anyhow::bail!("Failed to send log: {}", response.status());
        }

        Ok(())
    }
}
