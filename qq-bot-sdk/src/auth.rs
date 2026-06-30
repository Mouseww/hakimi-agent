use crate::error::{Error, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    pub access_token: String,
    #[serde(deserialize_with = "deserialize_expires_in")]
    pub expires_in: i64,
    #[serde(skip)]
    pub expires_at: Option<DateTime<Utc>>,
}

fn deserialize_expires_in<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse::<i64>().map_err(D::Error::custom)
}

impl AccessToken {
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() >= expires_at - Duration::seconds(60) // 提前60秒刷新
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub app_id: String,
    pub app_secret: String,
}

pub struct TokenManager {
    pub(crate) credentials: Credentials,
    token: Arc<RwLock<Option<AccessToken>>>,
    client: reqwest::Client,
    api_base: String,
}

impl TokenManager {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            credentials: Credentials { app_id, app_secret },
            token: Arc::new(RwLock::new(None)),
            client: reqwest::Client::new(),
            api_base: "https://bots.qq.com/app".to_string(),
        }
    }

    pub fn with_sandbox(mut self) -> Self {
        self.api_base = "https://sandbox.api.sgroup.qq.com".to_string();
        self
    }

    /// 获取有效 token，自动刷新过期 token
    pub async fn get_token(&self) -> Result<String> {
        {
            let token_guard = self.token.read();
            if let Some(token) = token_guard.as_ref() {
                if !token.is_expired() {
                    return Ok(token.access_token.clone());
                }
            }
        }

        self.refresh_token().await
    }

    /// 强制刷新 token
    pub async fn refresh_token(&self) -> Result<String> {
        let url = format!("{}/getAppAccessToken", self.api_base);
        tracing::info!("🔑 Requesting token from: {}", url);

        #[derive(Serialize)]
        struct TokenRequest {
            appId: String,
            clientSecret: String,
        }

        let resp = self
            .client
            .post(&url)
            .json(&TokenRequest {
                appId: self.credentials.app_id.clone(),
                clientSecret: self.credentials.app_secret.clone(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            tracing::error!("❌ Token request failed: {} - {}", status, text);
            return Err(Error::Auth(format!(
                "Token request failed: {} - {}",
                status, text
            )));
        }

        let body_text = resp.text().await?;
        tracing::info!("📥 Token response: {}", body_text);

        let mut token: AccessToken = serde_json::from_str(&body_text)?;
        token.expires_at = Some(Utc::now() + Duration::seconds(token.expires_in));

        let token_str = token.access_token.clone();
        tracing::info!(
            "✅ Got access_token (len={}): {}",
            token_str.len(),
            token_str
        );
        *self.token.write() = Some(token);

        Ok(token_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_expiry() {
        let mut token = AccessToken {
            access_token: "test".to_string(),
            expires_in: 7200,
            expires_at: Some(Utc::now() - Duration::seconds(100)),
        };
        assert!(token.is_expired());

        token.expires_at = Some(Utc::now() + Duration::seconds(3600));
        assert!(!token.is_expired());
    }
}
