//! Pushover.net message action

use reqwest::Url;
use sea_orm::Iterable;

use super::Action;
use crate::prelude::*;

#[allow(dead_code)]
// They request you make a request again after 5 seconds when a 5xx is returned, per <https://pushover.net/api#friendly>
const DEFAULT_PUSHOVER_RETRY_SECONDS: u64 = 5;

pub enum PushoverPriority {
    Lowest,
    Low,
    Normal,
    High,
    Emergency,
}

impl From<PushoverPriority> for i8 {
    fn from(priority: PushoverPriority) -> i8 {
        match priority {
            PushoverPriority::Lowest => -2,
            PushoverPriority::Low => -1,
            PushoverPriority::Normal => 0,
            PushoverPriority::High => 1,
            PushoverPriority::Emergency => 2,
        }
    }
}

/// Implements the Pushover action, API documentation is at <https://pushover.net/api#messages>
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct PushOver {
    /// API Token
    pub token: String,
    /// Specific user token
    pub user: String,
    /// Device name
    pub device: Option<String>,
    /// Message title
    pub title: Option<String>,
    /// Message to send, if you want to set one
    pub message: Option<String>,
    /// The states that this action will run on
    pub run_states: Vec<super::ServiceStatus>,

    /// current retry count
    #[serde(default)]
    retry_count: u8,
}

#[async_trait]
impl Action for PushOver {
    async fn execute(&self, check_result: &CheckResult) -> Result<(), Error> {
        if !self.run_states.contains(&check_result.status) {
            return Ok(());
        }

        let payload: PushoverMessage = PushoverMessage::from(self);

        debug!("Sending pushover payload: {:?}", payload);

        let client = reqwest::Client::new();
        let response = client
            .post("https://api.pushover.net/1/messages.json")
            .json(&payload)
            .send()
            .await?;

        if response.status().is_client_error() {
            todo!("Handle pushover client error")
            // panic!("Pushover returned a 4xx error, this is a bug");
        } else if response.status().is_server_error() {
            error!(
                "Pushover returned a 4xx error, retrying in {} seconds",
                DEFAULT_PUSHOVER_RETRY_SECONDS
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(
                DEFAULT_PUSHOVER_RETRY_SECONDS,
            ))
            .await;
            return self.execute(check_result).await;
        }
        dbg!(&response);

        let response_body = response.text().await?;

        let data: PushoverResponse = match serde_json::from_str(&response_body) {
            Ok(data) => data,
            Err(err) => {
                error!(
                    "Failed to parse pushover response: {:?} body={:?}",
                    err, response_body
                );
                return Err(Error::Generic(format!(
                    "Failed to parse pushover response: {:?}",
                    err
                )));
            }
        };

        if data.status != 1 {
            error!(
                "Failed to send pushover message: {:?}",
                data.errors.join(", ")
            );
            return Err(Error::Generic(format!(
                "Failed to send pushover message: {:?}",
                data.errors.join(", ")
            )));
        }
        Ok(())
    }
    fn run_states(&self) -> Vec<super::ServiceStatus> {
        if self.run_states.is_empty() {
            ServiceStatus::iter().collect::<Vec<_>>()
        } else {
            self.run_states.to_vec()
        }
    }
}

#[derive(Serialize, Debug)]
struct PushoverMessage {
    /// API Token
    pub token: String,
    /// Specific user token
    pub user: String,
    /// Device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Message title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Message to send
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Set a URL in the message
    pub url: Option<Url>,
    /// Set a URL title in the message
    pub url_title: Option<String>,
}

impl From<&PushOver> for PushoverMessage {
    fn from(pushover: &PushOver) -> Self {
        PushoverMessage {
            token: pushover.token.clone(),
            user: pushover.user.clone(),
            device: pushover.device.clone(),
            title: pushover.title.clone(),
            message: pushover.message.clone(),
            url_title: None,
            url: None,
        }
    }
}

#[derive(Deserialize, Debug, Default)]
struct PushoverResponse {
    status: i32,
    #[allow(dead_code)]
    user: Option<String>,
    #[allow(dead_code)]
    request: Option<Uuid>,
    /// Guess what's in here?
    #[serde(default)]
    errors: Vec<String>,
    /// If you set priority to Emergency, you'll get a receipt
    #[allow(dead_code)]
    receipt: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use crate::actions::{test_setup, Action, CheckResult, ServiceStatus};

    #[tokio::test]
    async fn test_pushover() {
        let token = match std::env::var("MAREMMA_TEST_PUSHOVER_TOKEN") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_PUSHOVER_TOKEN not set, skipping test");
                return;
            }
        };
        let user = match std::env::var("MAREMMA_TEST_PUSHOVER_USER") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("MAREMMA_TEST_PUSHOVER_USER not set, skipping test");
                return;
            }
        };

        let _ = test_setup().await.expect("Failed to setup test");

        let pushover = super::PushOver {
            token,
            user,
            device: None,
            title: None,
            message: Some(format!("test {}", chrono::Utc::now().timestamp())),
            run_states: vec![ServiceStatus::Critical],
            retry_count: 0,
        };

        let check_result = CheckResult {
            status: ServiceStatus::Critical,
            result_text: "result_text".to_string(),
            timestamp: chrono::Utc::now(),
            time_elapsed: TimeDelta::seconds(1),
        };

        pushover
            .execute(&check_result)
            .await
            .expect("Failed to send test pushover message");
    }
}
