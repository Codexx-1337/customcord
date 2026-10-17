use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Client;
use serde_json::json;

use crate::models::{Channel, Guild, Message, User};

const BASE_URL: &str = "https://discord.com/api/v10";
const CLIENT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

#[allow(dead_code)]
#[derive(Clone)]
pub struct DiscordRest {
    client: Client,
    token: String,
}

impl DiscordRest {
    pub fn new(token: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(CLIENT_USER_AGENT));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Ok(val) = HeaderValue::from_str(&token) {
            headers.insert(AUTHORIZATION, val);
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client, token }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn get_me(&self) -> Result<User, String> {
        let url = format!("{}/users/@me", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to request @me: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<User>()
            .await
            .map_err(|e| format!("Failed to parse User: {}", e))
    }

    pub async fn get_guilds(&self) -> Result<Vec<Guild>, String> {
        let url = format!("{}/users/@me/guilds", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to request guilds: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<Vec<Guild>>()
            .await
            .map_err(|e| format!("Failed to parse guilds: {}", e))
    }

    pub async fn get_dm_channels(&self) -> Result<Vec<Channel>, String> {
        let url = format!("{}/users/@me/channels", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to request DM channels: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<Vec<Channel>>()
            .await
            .map_err(|e| format!("Failed to parse DM channels: {}", e))
    }

    pub async fn get_guild_channels(&self, guild_id: &str) -> Result<Vec<Channel>, String> {
        let url = format!("{}/guilds/{}/channels", BASE_URL, guild_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to request channels: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<Vec<Channel>>()
            .await
            .map_err(|e| format!("Failed to parse channels: {}", e))
    }

    pub async fn get_channel_messages(&self, channel_id: &str, limit: u32) -> Result<Vec<Message>, String> {
        let url = format!("{}/channels/{}/messages?limit={}", BASE_URL, channel_id, limit);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to request messages: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<Vec<Message>>()
            .await
            .map_err(|e| format!("Failed to parse messages: {}", e))
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<Message, String> {
        let url = format!("{}/channels/{}/messages", BASE_URL, channel_id);
        let payload = json!({
            "content": content
        });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send message: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Discord API error ({}): {}", status, body));
        }

        resp.json::<Message>()
            .await
            .map_err(|e| format!("Failed to parse sent message: {}", e))
    }
}
