use crate::types::{LogDriver, Message};

use anyhow::Result;
use axum_prometheus::metrics::counter;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

pub struct Controller {
    _sender: mpsc::UnboundedSender<Message>,
    receiver: mpsc::UnboundedReceiver<Message>,
    drivers: Vec<Box<dyn LogDriver>>,
    processed_messages: usize,
}

impl Controller {
    pub fn new(
        _sender: mpsc::UnboundedSender<Message>,
        receiver: mpsc::UnboundedReceiver<Message>,
        drivers: Vec<Box<dyn LogDriver>>,
    ) -> Self {
        Self {
            _sender,
            receiver,
            drivers,
            processed_messages: 0,
        }
    }

    pub async fn init(&mut self) -> Result<()> {
        for driver in &mut self.drivers {
            driver.init().await?;
        }
        info!("All drivers initialized");
        Ok(())
    }

    pub async fn run(&mut self) {
        info!("waiting for logs to send to drivers...");
        while let Some(message) = self.receiver.recv().await {
            let id = message.deployment_id.clone();
            debug!(?id, "processing message...");
            match self.handle_message(&message).await {
                Ok(_) => {
                    debug!(?id, "message handled successfully");
                    self.processed_messages += 1;
                    counter!("drain_processed_messages").increment(1);
                }
                Err(e) => {
                    error!(?id, "failed to handle message: {:?}", e);
                    counter!("drain_failed_messages").increment(1);
                }
            }
            if self.processed_messages % 100 == 0 {
                info!(
                    processed_messages = self.processed_messages,
                    "processed 100 messages..."
                );
            }
        }
    }

    async fn handle_message(&mut self, message: &Message) -> Result<()> {
        for driver in &mut self.drivers {
            if let Err(e) = driver.send_log(message).await {
                error!("Failed to send log to driver: {:?}", e);
            }
        }
        Ok(())
    }
}
