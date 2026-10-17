use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub discriminator: Option<String>,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl User {
    pub fn display_name(&self) -> &str {
        if let Some(ref gn) = self.global_name {
            if !gn.is_empty() {
                return gn.as_str();
            }
        }
        &self.username
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub url: String,
    pub proxy_url: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub guild_id: Option<String>,
    pub author: User,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    GuildText = 0,
    Dm = 1,
    GuildVoice = 2,
    GroupDm = 3,
    GuildCategory = 4,
    GuildAnnouncement = 5,
    GuildStageVoice = 13,
    GuildForum = 15,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    pub guild_id: Option<String>,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub position: Option<i32>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub recipients: Vec<User>,
}

impl Channel {
    pub fn display_name(&self, current_user_id: &str) -> String {
        if let Some(ref name) = self.name {
            if !name.is_empty() {
                return name.clone();
            }
        }
        if !self.recipients.is_empty() {
            let names: Vec<String> = self
                .recipients
                .iter()
                .filter(|u| u.id != current_user_id)
                .map(|u| u.display_name().to_string())
                .collect();
            if !names.is_empty() {
                return names.join(", ");
            }
        }
        "Unnamed Channel".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub unavailable: Option<bool>,
}

impl Guild {
    pub fn initials(&self) -> String {
        let words: Vec<&str> = self.name.split_whitespace().collect();
        if words.is_empty() {
            return "?".to_string();
        }
        if words.len() == 1 {
            words[0].chars().take(3).collect()
        } else {
            words.iter().filter_map(|w| w.chars().next()).take(3).collect()
        }
    }
}
