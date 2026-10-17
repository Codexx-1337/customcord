use std::collections::HashMap;
use egui::{CentralPanel, Context, SidePanel};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

use crate::config::AppConfig;
use crate::discord::{ClientEvent, DiscordRest, GatewayClient};
use crate::models::{Channel, Guild, Message, User};
use crate::ui::channel_sidebar::ChannelSidebar;
use crate::ui::chat_view::ChatView;
use crate::ui::login_view::LoginView;
use crate::ui::server_bar::ServerBar;
use crate::ui::theme::DiscordTheme;

#[derive(Debug)]
pub enum UiEvent {
    Gateway(ClientEvent),
    GuildsLoaded(Vec<Guild>),
    DirectMessagesLoaded(Vec<Channel>),
    ChannelMessagesLoaded { channel_id: String, messages: Vec<Message> },
    GuildChannelsLoaded { guild_id: String, channels: Vec<Channel> },
    MessageSent(Message),
    ActionError(String),
}

pub struct LightcordApp {
    config: AppConfig,
    login_view: LoginView,
    chat_view: ChatView,

    // Session state
    is_authenticated: bool,
    current_user: Option<User>,
    guilds: Vec<Guild>,
    private_channels: Vec<Channel>,
    guild_channels: HashMap<String, Vec<Channel>>,
    messages: HashMap<String, Vec<Message>>,

    selected_guild_id: Option<String>,
    selected_channel_id: Option<String>,

    rest_client: Option<DiscordRest>,
    event_tx: UnboundedSender<UiEvent>,
    event_rx: UnboundedReceiver<UiEvent>,
    gateway_handle: Option<tokio::task::JoinHandle<()>>,
}

impl LightcordApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        DiscordTheme::apply(&_cc.egui_ctx);

        let config = AppConfig::load();
        let (event_tx, event_rx) = unbounded_channel::<UiEvent>();

        let mut app = Self {
            login_view: LoginView::new(config.token.clone(), config.save_token),
            chat_view: ChatView::new(),
            config,
            is_authenticated: false,
            current_user: None,
            guilds: Vec::new(),
            private_channels: Vec::new(),
            guild_channels: HashMap::new(),
            messages: HashMap::new(),
            selected_guild_id: None,
            selected_channel_id: None,
            rest_client: None,
            event_tx,
            event_rx,
            gateway_handle: None,
        };

        // If auto-login or saved token is available
        if app.config.save_token && !app.config.token.trim().is_empty() {
            app.connect(app.config.token.clone(), &_cc.egui_ctx);
        }

        app
    }

    pub fn connect(&mut self, token: String, ctx: &Context) {
        info!("Connecting with token...");
        let rest = DiscordRest::new(token.clone());
        self.rest_client = Some(rest.clone());

        // Cancel previous gateway if any
        if let Some(handle) = self.gateway_handle.take() {
            handle.abort();
        }

        let (gw_tx, mut gw_rx) = unbounded_channel::<ClientEvent>();
        let ui_tx = self.event_tx.clone();
        let ctx_clone = ctx.clone();

        // Gateway forwarder task
        tokio::spawn(async move {
            while let Some(ev) = gw_rx.recv().await {
                let _ = ui_tx.send(UiEvent::Gateway(ev));
                ctx_clone.request_repaint();
            }
        });

        // Gateway runner
        let client = GatewayClient::new(token.clone(), gw_tx);
        self.gateway_handle = Some(tokio::spawn(async move {
            client.run().await;
        }));

        // Fallback & fast initial loading: fetch guilds and DMs via REST API
        let rest_guilds = rest.clone();
        let ui_tx_guilds = self.event_tx.clone();
        let ctx_guilds = ctx.clone();
        tokio::spawn(async move {
            match rest_guilds.get_guilds().await {
                Ok(guilds) => {
                    let _ = ui_tx_guilds.send(UiEvent::GuildsLoaded(guilds));
                    ctx_guilds.request_repaint();
                }
                Err(e) => {
                    error!("Failed to fetch guilds via REST: {}", e);
                }
            }
        });

        let rest_dms = rest.clone();
        let ui_tx_dms = self.event_tx.clone();
        let ctx_dms = ctx.clone();
        tokio::spawn(async move {
            match rest_dms.get_dm_channels().await {
                Ok(dms) => {
                    let _ = ui_tx_dms.send(UiEvent::DirectMessagesLoaded(dms));
                    ctx_dms.request_repaint();
                }
                Err(e) => {
                    error!("Failed to fetch DMs via REST: {}", e);
                }
            }
        });
    }

    pub fn logout(&mut self) {
        if let Some(handle) = self.gateway_handle.take() {
            handle.abort();
        }
        self.is_authenticated = false;
        self.current_user = None;
        self.guilds.clear();
        self.private_channels.clear();
        self.guild_channels.clear();
        self.messages.clear();
        self.selected_guild_id = None;
        self.selected_channel_id = None;
        self.rest_client = None;
        self.login_view.is_loading = false;
        self.login_view.status_message = None;
    }

    fn load_channel_messages(&mut self, channel_id: String, ctx: &Context) {
        if self.messages.contains_key(&channel_id) {
            return;
        }

        if let Some(ref rest) = self.rest_client {
            let rest = rest.clone();
            let ui_tx = self.event_tx.clone();
            let ctx = ctx.clone();
            let ch_id = channel_id.clone();

            tokio::spawn(async move {
                match rest.get_channel_messages(&ch_id, 50).await {
                    Ok(mut msgs) => {
                        // Discord returns messages newest first; reverse so oldest is top
                        msgs.reverse();
                        let _ = ui_tx.send(UiEvent::ChannelMessagesLoaded {
                            channel_id: ch_id,
                            messages: msgs,
                        });
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        error!("Failed to fetch messages for channel {}: {}", ch_id, e);
                    }
                }
            });
        }
    }

    fn load_guild_channels(&mut self, guild_id: String, ctx: &Context) {
        if self.guild_channels.contains_key(&guild_id) {
            return;
        }

        if let Some(ref rest) = self.rest_client {
            let rest = rest.clone();
            let ui_tx = self.event_tx.clone();
            let ctx = ctx.clone();
            let g_id = guild_id.clone();

            tokio::spawn(async move {
                match rest.get_guild_channels(&g_id).await {
                    Ok(channels) => {
                        let _ = ui_tx.send(UiEvent::GuildChannelsLoaded {
                            guild_id: g_id,
                            channels,
                        });
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        error!("Failed to fetch channels for guild {}: {}", g_id, e);
                    }
                }
            });
        }
    }

    fn send_message(&mut self, channel_id: String, content: String, ctx: &Context) {
        if let Some(ref rest) = self.rest_client {
            let rest = rest.clone();
            let ui_tx = self.event_tx.clone();
            let ctx = ctx.clone();
            let ch_id = channel_id.clone();

            tokio::spawn(async move {
                match rest.send_message(&ch_id, &content).await {
                    Ok(msg) => {
                        let _ = ui_tx.send(UiEvent::MessageSent(msg));
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        error!("Failed to send message: {}", e);
                        let _ = ui_tx.send(UiEvent::ActionError(format!("Ошибка отправки: {}", e)));
                        ctx.request_repaint();
                    }
                }
            });
        }
    }

    fn handle_events(&mut self, ctx: &Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                UiEvent::Gateway(ClientEvent::Ready {
                    user,
                    guilds,
                    private_channels,
                }) => {
                    self.is_authenticated = true;
                    self.login_view.is_loading = false;
                    self.login_view.status_message = None;

                    // Extract channels from initial guilds if present
                    for g in &guilds {
                        if !g.channels.is_empty() {
                            self.guild_channels.insert(g.id.clone(), g.channels.clone());
                        }
                    }

                    self.current_user = Some(user);
                    self.guilds = guilds;
                    self.private_channels = private_channels;

                    // Save config if requested
                    if self.login_view.save_token {
                        self.config.token = self.login_view.token.clone();
                        self.config.save_token = true;
                        let _ = self.config.save();
                    }

                    // Auto select first DM if available
                    if self.selected_channel_id.is_none() && !self.private_channels.is_empty() {
                        let ch_id = self.private_channels[0].id.clone();
                        self.selected_channel_id = Some(ch_id.clone());
                        self.load_channel_messages(ch_id, ctx);
                    }
                }
                UiEvent::GuildsLoaded(guilds) => {
                    info!("Loaded {} guilds via REST", guilds.len());
                    for g in guilds {
                        if !self.guilds.iter().any(|existing| existing.id == g.id) {
                            self.guilds.push(g);
                        }
                    }
                }
                UiEvent::DirectMessagesLoaded(dms) => {
                    info!("Loaded {} DMs via REST", dms.len());
                    self.private_channels = dms;
                    if self.selected_channel_id.is_none() && !self.private_channels.is_empty() {
                        let ch_id = self.private_channels[0].id.clone();
                        self.selected_channel_id = Some(ch_id.clone());
                        self.load_channel_messages(ch_id, ctx);
                    }
                }
                UiEvent::Gateway(ClientEvent::MessageCreate(msg)) => {
                    let ch_id = msg.channel_id.clone();
                    let list = self.messages.entry(ch_id.clone()).or_default();
                    // Avoid duplicate if we already have it
                    if !list.iter().any(|m| m.id == msg.id) {
                        list.push(msg);
                    }
                }
                UiEvent::Gateway(ClientEvent::GuildCreate(guild)) => {
                    if !guild.channels.is_empty() {
                        self.guild_channels.insert(guild.id.clone(), guild.channels.clone());
                    }
                    if let Some(pos) = self.guilds.iter().position(|g| g.id == guild.id) {
                        self.guilds[pos] = guild;
                    } else {
                        self.guilds.push(guild);
                    }
                }
                UiEvent::Gateway(ClientEvent::Error(err)) => {
                    self.login_view.is_loading = false;
                    self.login_view.status_message = Some(err);
                }
                UiEvent::Gateway(ClientEvent::Disconnected(reason)) => {
                    info!("Disconnected: {}", reason);
                }
                UiEvent::ChannelMessagesLoaded { channel_id, messages } => {
                    self.messages.insert(channel_id, messages);
                }
                UiEvent::GuildChannelsLoaded { guild_id, channels } => {
                    self.guild_channels.insert(guild_id, channels);
                }
                UiEvent::MessageSent(msg) => {
                    let ch_id = msg.channel_id.clone();
                    let list = self.messages.entry(ch_id).or_default();
                    if !list.iter().any(|m| m.id == msg.id) {
                        list.push(msg);
                    }
                }
                UiEvent::ActionError(err) => {
                    error!("Action error: {}", err);
                }
                _ => {}
            }
        }
    }
}

impl eframe::App for LightcordApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);

        if !self.is_authenticated {
            CentralPanel::default().show(ctx, |ui| {
                if let Some(token) = self.login_view.render(ui) {
                    self.connect(token, ctx);
                }
            });
            return;
        }

        // Left Server Rail
        let mut select_home = false;
        let mut select_guild_id = None;

        SidePanel::left("server_rail")
            .resizable(false)
            .exact_width(ServerBar::WIDTH)
            .show(ctx, |ui| {
                ServerBar::render(
                    ui,
                    &self.guilds,
                    &self.selected_guild_id,
                    || select_home = true,
                    |gid| select_guild_id = Some(gid.to_string()),
                );
            });

        if select_home {
            self.selected_guild_id = None;
            if let Some(first_dm) = self.private_channels.first() {
                self.selected_channel_id = Some(first_dm.id.clone());
                self.load_channel_messages(first_dm.id.clone(), ctx);
            }
        } else if let Some(gid) = select_guild_id {
            self.selected_guild_id = Some(gid.clone());
            self.load_guild_channels(gid.clone(), ctx);

            // Auto select first text channel in guild if available
            if let Some(chs) = self.guild_channels.get(&gid) {
                if let Some(first_text) = chs.iter().find(|c| c.channel_type == crate::models::ChannelType::GuildText) {
                    self.selected_channel_id = Some(first_text.id.clone());
                    self.load_channel_messages(first_text.id.clone(), ctx);
                }
            }
        }

        // Second sidebar: Channels or Direct Messages
        let mut select_channel_id = None;
        let mut logout_requested = false;

        let selected_guild = self
            .selected_guild_id
            .as_ref()
            .and_then(|gid| self.guilds.iter().find(|g| &g.id == gid))
            .cloned();

        let current_channels = self
            .selected_guild_id
            .as_ref()
            .and_then(|gid| self.guild_channels.get(gid))
            .cloned()
            .unwrap_or_default();

        SidePanel::left("channel_sidebar")
            .resizable(false)
            .exact_width(ChannelSidebar::WIDTH)
            .show(ctx, |ui| {
                ChannelSidebar::render(
                    ui,
                    self.current_user.as_ref(),
                    selected_guild.as_ref(),
                    &current_channels,
                    &self.private_channels,
                    self.selected_channel_id.as_deref(),
                    |cid| select_channel_id = Some(cid.to_string()),
                    || logout_requested = true,
                );
            });

        if logout_requested {
            self.logout();
            return;
        }

        if let Some(cid) = select_channel_id {
            self.selected_channel_id = Some(cid.clone());
            self.load_channel_messages(cid, ctx);
        }

        // Main Area: Chat & Input
        let current_channel = self.selected_channel_id.as_ref().and_then(|cid| {
            if self.selected_guild_id.is_some() {
                current_channels.iter().find(|c| &c.id == cid)
            } else {
                self.private_channels.iter().find(|c| &c.id == cid)
            }
        });

        let current_user_id = self.current_user.as_ref().map(|u| u.id.as_str()).unwrap_or("");
        let empty_msgs = Vec::new();
        let messages = self
            .selected_channel_id
            .as_ref()
            .and_then(|cid| self.messages.get(cid))
            .unwrap_or(&empty_msgs);

        let mut send_text = None;

        CentralPanel::default().show(ctx, |ui| {
            self.chat_view.render(
                ui,
                current_channel,
                current_user_id,
                messages,
                |text| send_text = Some(text),
            );
        });

        if let Some(text) = send_text {
            if let Some(ref cid) = self.selected_channel_id {
                self.send_message(cid.clone(), text, ctx);
            }
        }
    }
}
