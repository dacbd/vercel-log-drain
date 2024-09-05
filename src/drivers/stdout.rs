use crate::types::{LogDriver, Message};
use anyhow::Result;
use async_trait::async_trait;
use tracing::debug;

pub struct StdOutDriver;

impl StdOutDriver {
    pub fn new() -> Self {
        Self {}
    }
}
#[async_trait]
impl LogDriver for StdOutDriver {
    async fn init(&mut self) -> Result<()> {
        debug!("init stdout driver");
        Ok(())
    }

    async fn send_log(&mut self, message: &Message) -> Result<()> {
        let log_line = serde_json::to_string(message)?;
        println!("{}", log_line);
        Ok(())
    }
}
