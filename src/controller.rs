use crate::types::Message;

use std::collections::HashSet;

use anyhow::Result;
use aws_sdk_cloudwatchlogs::{
    operation::{
        create_log_group::CreateLogGroupError, create_log_stream::CreateLogStreamError,
        put_log_events::PutLogEventsError,
    },
    types::InputLogEvent,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

pub struct Controller {
    _sender: mpsc::UnboundedSender<Message>,
    receiver: mpsc::UnboundedReceiver<Message>,
    cwl_client: aws_sdk_cloudwatchlogs::Client,
    groups: HashSet<String>,
    streams: HashSet<String>,
    processed_messages: usize,
}

impl Controller {
    pub fn new(
        _sender: mpsc::UnboundedSender<Message>,
        receiver: mpsc::UnboundedReceiver<Message>,
        cwl_client: aws_sdk_cloudwatchlogs::Client,
    ) -> Self {
        Self {
            _sender,
            receiver,
            cwl_client,
            groups: HashSet::new(),
            streams: HashSet::new(),
            processed_messages: 0,
        }
    }
    pub async fn init_aws_state(&mut self) -> Result<()> {
        // load "/vercel/*" log groups
        let mut log_groups = self
            .cwl_client
            .describe_log_groups()
            .set_log_group_name_prefix(Some(String::from("/vercel/")))
            .into_paginator()
            .send();

        while let Some(result) = log_groups.next().await {
            match result {
                Ok(response) => {
                    if let Some(groups) = response.log_groups {
                        groups
                            .iter()
                            .filter_map(|lg| lg.log_group_name().map(String::from))
                            .for_each(|str| {
                                self.groups.insert(str);
                            });
                    }
                }
                Err(error) => {
                    error!("error describing log groups: {:?}", error);
                    return Err(error.into());
                }
            }
        }
        let page_streams = self.groups.iter().map(|group| {
            self.cwl_client
                .describe_log_streams()
                .set_log_group_name(Some(group.clone()))
                .into_paginator()
                .send()
        });
        for mut page_stream in page_streams {
            while let Some(result) = page_stream.next().await {
                match result {
                    Ok(response) => {
                        if let Some(streams) = response.log_streams {
                            for stream in streams {
                                if let Some(stream_name) = stream.log_stream_name() {
                                    debug!("reveived log stream: {}", stream_name);
                                    self.streams.insert(String::from(stream_name));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        error!("error describing log streams: {:?}", error);
                        return Err(error.into());
                    }
                }
            }
        }
        info!(
            log_group_count = self.groups.len(),
            log_stream_count = self.streams.len(),
            "aws state initialized"
        );
        return Ok(());
    }
    pub async fn run(&mut self) {
        info!("waiting for logs to send to aws...");
        while let Some(message) = self.receiver.recv().await {
            let id = message.deployment_id.clone();
            debug!(?id, "processing message...");
            match self.handle_message(message).await {
                Ok(_) => {
                    debug!(?id, "message handled successfully");
                    self.processed_messages += 1;
                }
                Err(e) => {
                    error!(?id, "failed to handle message: {:?}", e);
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
    fn seen_group(&self, group_name: &str) -> bool {
        self.groups.contains(group_name)
    }
    fn seen_stream(&self, stream_name: &str) -> bool {
        self.streams.contains(stream_name)
    }
    async fn create_group(&mut self, group_name: &str) -> Result<()> {
        match self
            .cwl_client
            .create_log_group()
            .set_log_group_name(Some(group_name.to_owned()))
            .send()
            .await
        {
            Ok(_) => {
                info!("created log group: {}", group_name);
            }
            Err(e) => match e.into_service_error() {
                CreateLogGroupError::ResourceAlreadyExistsException(_) => {}
                inner_err => {
                    error!(?group_name, "failed to create log group: {:?}", inner_err);
                }
            },
        }

        self.groups.insert(group_name.to_owned());
        match self
            .cwl_client
            .put_retention_policy()
            .log_group_name(group_name)
            .retention_in_days(90)
            .send()
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!(?group_name, "failed to set retention policy: {:?}", e);
            }
        }
        return Ok(());
    }
    async fn create_stream(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        match self
            .cwl_client
            .create_log_stream()
            .set_log_group_name(Some(group_name.to_owned()))
            .set_log_stream_name(Some(stream_name.to_owned()))
            .send()
            .await
        {
            Ok(_) => {
                info!("created log stream: {}", stream_name);
            }
            Err(e) => match e.into_service_error() {
                CreateLogStreamError::ResourceAlreadyExistsException(_) => {
                    info!("caching log stream: {} (other node created)", stream_name);
                }
                inner_err => {
                    error!(
                        ?group_name,
                        ?stream_name,
                        "failed to create log stream: {:?}",
                        inner_err
                    );
                    return Err(inner_err.into());
                }
            },
        }
        self.streams.insert(stream_name.to_owned());
        return Ok(());
    }
    async fn check_or_create_group(&mut self, group_name: &str) -> Result<()> {
        if !self.seen_group(group_name) {
            self.create_group(group_name).await?;
        }
        return Ok(());
    }
    async fn check_or_create_stream(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        if !self.seen_stream(stream_name) {
            self.create_stream(group_name, stream_name).await?;
        }
        return Ok(());
    }
    async fn check_or_create(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        self.check_or_create_group(group_name).await?;
        self.check_or_create_stream(group_name, stream_name).await?;
        return Ok(());
    }
    async fn handle_message(&mut self, message: Message) -> Result<()> {
        let mut retries: usize = 0;
        let group_name = format!("/vercel/{}/{}", message.project_name, message.source);
        #[allow(clippy::useless_format)]
        let stream_name = format!("{}", message.deployment_id);
        self.check_or_create(&group_name, &stream_name).await?;

        let payload = serde_json::to_string(&message)?;
        let log_event = InputLogEvent::builder()
            .timestamp(message.timestamp)
            .message(payload)
            .build()?;

        while let Err(e) = self
            .cwl_client
            .put_log_events()
            .log_group_name(&group_name)
            .log_stream_name(&stream_name)
            .log_events(log_event.clone())
            .send()
            .await
        {
            retries += 1;
            if retries > 5 {
                warn!(
                    ?group_name,
                    ?stream_name,
                    ?log_event,
                    error = ?e,
                    "failed to put log event after 5 retries"
                );
                return Err(e.into());
            }
            match e.into_service_error() {
                PutLogEventsError::ResourceNotFoundException(_) => {
                    warn!("log group or stream not found, trying to create again...");
                    self.create_group(&group_name).await?;
                    self.create_stream(&group_name, &stream_name).await?;
                }
                inner_err => {
                    error!(
                        ?group_name,
                        ?stream_name,
                        "failed to put log event: {:?}",
                        inner_err
                    );
                }
            }
            info!(id = message.id, ?retries, "retrying message...");
        }
        return Ok(());
    }
}
