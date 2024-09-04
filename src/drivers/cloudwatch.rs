use crate::types::{LogDriver, Message};
use anyhow::Result;
use async_trait::async_trait;
use aws_sdk_cloudwatchlogs::{
    error::SdkError,
    operation::{
        create_log_group::CreateLogGroupError, create_log_stream::CreateLogStreamError,
        put_log_events::PutLogEventsError,
    },
    types::InputLogEvent,
};
use core::result::Result::Ok;
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

pub struct CloudWatchDriver {
    client: aws_sdk_cloudwatchlogs::Client,
    groups: HashSet<String>,
    streams: HashSet<String>,
}

impl CloudWatchDriver {
    pub fn new(client: aws_sdk_cloudwatchlogs::Client) -> Self {
        Self {
            client,
            groups: HashSet::new(),
            streams: HashSet::new(),
        }
    }
    async fn create_group(&mut self, group_name: &str) -> Result<()> {
        match self
            .client
            .create_log_group()
            .set_log_group_name(Some(group_name.to_owned()))
            .send()
            .await
            .map_err(SdkError::into_service_error)
        {
            Ok(_) => {
                info!(?group_name, "created log group");
            }
            Err(CreateLogGroupError::ResourceAlreadyExistsException(_)) => {
                info!(?group_name, "log group already exists");
            }
            Err(e) => {
                error!(?group_name, "failed to create log group: {e:?}");
                return Err(e.into());
            }
        }

        self.groups.insert(group_name.to_owned());
        if let Err(e) = self
            .client
            .put_retention_policy()
            .log_group_name(group_name)
            .retention_in_days(90)
            .send()
            .await
            .map_err(SdkError::into_service_error)
        {
            warn!(?group_name, "failed to set retention policy: {:?}", e);
        }

        return Ok(());
    }

    async fn create_stream(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        match self
            .client
            .create_log_stream()
            .set_log_group_name(Some(group_name.to_owned()))
            .set_log_stream_name(Some(stream_name.to_owned()))
            .send()
            .await
            .map_err(SdkError::into_service_error)
        {
            Ok(_) => {
                info!("created log stream: {}", stream_name);
            }

            Err(CreateLogStreamError::ResourceAlreadyExistsException(_)) => {
                info!("caching log stream: {} (other node created)", stream_name);
            }

            Err(e) => {
                error!(
                    ?group_name,
                    ?stream_name,
                    "failed to create log stream: {e:?}",
                );
                return Err(e.into());
            }
        }
        self.streams.insert(stream_name.to_owned());
        return Ok(());
    }

    async fn check_or_create_group(&mut self, group_name: &str) -> Result<()> {
        if !self.groups.contains(group_name) {
            self.create_group(group_name).await?;
        }
        Ok(())
    }
    async fn check_or_create_stream(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        if !self.streams.contains(stream_name) {
            self.create_stream(group_name, stream_name).await?;
        }
        Ok(())
    }
    async fn check_or_create(&mut self, group_name: &str, stream_name: &str) -> Result<()> {
        self.check_or_create_group(group_name).await?;
        self.check_or_create_stream(group_name, stream_name).await?;
        Ok(())
    }
}

#[async_trait]
impl LogDriver for CloudWatchDriver {
    async fn init(&mut self) -> Result<()> {
        // load "/vercel/*" log groups
        let mut log_groups = self
            .client
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
            self.client
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

    async fn send_log(&mut self, message: &Message) -> Result<()> {
        let mut retries: usize = 0;
        let project_name = match &message.project_name {
            Some(n) => n.as_str(),
            None => "null",
        };
        let group_name = format!("/vercel/{project_name}/{}", message.source);
        #[allow(clippy::useless_format)]
        let stream_name = format!("{}", message.deployment_id);
        self.check_or_create(&group_name, &stream_name).await?;

        let payload = serde_json::to_string(&message)?;
        let log_event = InputLogEvent::builder()
            .timestamp(message.timestamp)
            .message(payload)
            .build()?;

        while let Err(e) = self
            .client
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
