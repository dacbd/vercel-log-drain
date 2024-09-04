use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct AppState {
    pub vercel_verify: String,
    pub vercel_secret: ring::hmac::Key,
    pub log_queue: tokio::sync::mpsc::UnboundedSender<Message>,
}

#[derive(Deserialize, Debug)]
pub struct VercelPayload(pub Vec<Message>);

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    #[serde(deserialize_with = "deserialize_message_data")]
    #[serde(default)]
    pub message: serde_json::Value,
    pub timestamp: i64,
    #[serde(rename = "type")]
    pub output_type: Option<String>,
    pub source: String,
    // projectName is not set on the test payload
    pub project_name: Option<String>,
    pub project_id: String,
    pub deployment_id: String,
    pub build_id: Option<String>,
    pub host: String,
    pub path: Option<String>,
    pub entrypoint: Option<String>,
    pub request_id: Option<String>,
    #[allow(private_interfaces)]
    pub proxy: Option<VercelProxy>,
}

fn deserialize_message_data<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let ref_value = value.clone();
    match value {
        serde_json::Value::String(str) => {
            let result = serde_json::Value::from_str(&str);
            match result {
                Ok(nested_value) => Ok(nested_value),
                Err(_) => Ok(ref_value),
            }
        }
        _ => Ok(value),
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct VercelProxy {
    timestamp: i64,
    method: String,
    scheme: String,
    host: String,
    user_agent: Vec<String>,
    referer: Option<String>,
    status_code: Option<isize>,
    client_ip: String,
    region: String,
    cache_id: Option<String>,
    vercel_cache: Option<String>,
}

#[async_trait]
pub trait LogDriver: Send + Sync {
    async fn init(&mut self) -> Result<()>;
    async fn send_log(&mut self, message: &Message) -> Result<()>;
}

#[cfg(test)]
mod test {
    #[test]
    fn all_seen_messages_parse_ok() {
        let test_data = vec![
            include_str!("fixtures/sample_1.json"),
            include_str!("fixtures/sample_2.json"),
            include_str!("fixtures/sample_3.json"),
            include_str!("fixtures/sample_4.json"),
            include_str!("fixtures/sample_5.json"),
            // Vercel's test requests, missing projectName field
            include_str!("fixtures/test_build.json"),
            include_str!("fixtures/test_edge.json"),
            include_str!("fixtures/test_lambda.json"),
            include_str!("fixtures/test_static.json"),
        ];
        for data in test_data {
            let result = serde_json::from_str::<super::VercelPayload>(data);
            assert!(result.is_ok());
        }
    }
    #[test]
    fn parses_structured_messages() {
        let test_data = [
            include_str!("fixtures/structured_message_1.json"),
            include_str!("fixtures/structured_message_2.json"),
            include_str!("fixtures/sample_1.json"),
        ];
        for (index, data) in test_data.iter().enumerate() {
            let result = serde_json::from_str::<super::VercelPayload>(data);
            assert!(result.is_ok());
            let payload = result.unwrap().0;
            for msg in payload {
                match index {
                    0 => assert!(msg.message.is_object()),
                    1 => assert!(msg.message.is_object()),
                    _ => assert!(msg.message.is_string() || msg.message.is_null()),
                }
            }
        }
    }
    #[test]
    fn parses_structured_data_as_expected() {
        let test_data = include_str!("fixtures/structured_message_1.json");
        let result = serde_json::from_str::<super::VercelPayload>(test_data);
        assert!(result.is_ok());
        let payload = result.unwrap();
        let msg = payload.0.first().unwrap();
        let message = &msg.message;
        match message {
            serde_json::Value::Object(obj) => {
                assert_eq!(obj.get("logType").unwrap().as_str().unwrap(), "location");
            }
            _ => {
                unreachable!("expected structured message")
            }
        }
    }
}
