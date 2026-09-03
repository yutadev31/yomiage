use std::{collections::HashMap, env, sync::Arc};

use serenity::{
    all::{ChannelId, EventHandler, GatewayIntents, GuildId, Interaction, Message, Ready, UserId},
    async_trait,
    prelude::*,
};
use songbird::SerenityInit;
use tokio::{
    sync::{Semaphore, mpsc, watch},
    task::AbortHandle,
};

const MY_SERVER: u64 = 1544917245472284774;
pub(crate) const MESSAGE_QUEUE_CAPACITY: usize = 32;
const MAX_CONCURRENT_SYNTHESIS: usize = 2;

mod chunking;
mod commands;
mod voicevox;

pub struct BotState {
    pub playback: HashMap<GuildId, GuildPlayback>,
    pub voicevox: voicevox::Voicevox,
    pub synthesis_permits: Arc<Semaphore>,
    pub user_settings: HashMap<UserId, SpeechSettings>,
}

pub struct GuildPlayback {
    pub text_channel_id: ChannelId,
    pub sender: mpsc::Sender<SpeechRequest>,
    pub skip_sender: watch::Sender<u64>,
    pub task: AbortHandle,
}

#[derive(Clone, Default)]
pub struct SpeechSettings {
    pub speaker: Option<u32>,
    pub speed: Option<f64>,
}

pub struct SpeechRequest {
    pub text: String,
    pub settings: SpeechSettings,
}

pub struct BotStateKey;

impl TypeMapKey for BotStateKey {
    type Value = Arc<RwLock<BotState>>;
}

pub async fn bot_state(ctx: &Context) -> Arc<RwLock<BotState>> {
    let data = ctx.data.read().await;
    data.get::<BotStateKey>()
        .expect("Bot state is not initialized")
        .clone()
}

struct Handler;

fn bot_is_alone(ctx: &Context, guild_id: GuildId) -> bool {
    let bot_id = ctx.cache.current_user().id;
    let Some(guild) = ctx.cache.guild(guild_id) else {
        return false;
    };
    let Some(bot_channel_id) = guild
        .voice_states
        .get(&bot_id)
        .and_then(|voice_state| voice_state.channel_id)
    else {
        return false;
    };

    !guild.voice_states.values().any(|voice_state| {
        voice_state.channel_id == Some(bot_channel_id)
            && voice_state.user_id != bot_id
            && voice_state
                .member
                .as_ref()
                .map(|member| !member.user.bot)
                // If member data is not cached, keep the call alive rather than
                // accidentally disconnecting while someone is present.
                .unwrap_or(true)
    })
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Logged in as {}", ready.user.name);

        let guild_id = GuildId::new(MY_SERVER);

        guild_id
            .create_command(&ctx, commands::ping::register())
            .await
            .unwrap();

        guild_id
            .create_command(&ctx, commands::join::register())
            .await
            .unwrap();

        guild_id
            .create_command(&ctx, commands::leave::register())
            .await
            .unwrap();

        guild_id
            .create_command(&ctx, commands::settings::register_speed())
            .await
            .unwrap();

        guild_id
            .create_command(&ctx, commands::settings::register_speaker())
            .await
            .unwrap();

        guild_id
            .create_command(&ctx, commands::skip::register())
            .await
            .unwrap();
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if msg.content.trim_start().starts_with(';') {
            return;
        }

        let Some(guild_id) = msg.guild_id else {
            return;
        };

        let bot_id = ctx.cache.current_user().id;
        if msg.mentions.iter().any(|user| user.id == bot_id) {
            let response =
                match commands::join::join(&ctx, guild_id, msg.author.id, msg.channel_id).await {
                    Ok(()) => "接続しました。".to_owned(),
                    Err(message) => message,
                };

            if let Err(err) = msg.channel_id.say(&ctx.http, response).await {
                eprintln!("Failed to respond to join mention: {err}");
            }
            return;
        }

        let state = bot_state(&ctx).await;

        let (sender, settings) = {
            let state = state.read().await;
            let Some(playback) = state.playback.get(&guild_id) else {
                return;
            };
            if msg.channel_id != playback.text_channel_id {
                return;
            }
            (
                playback.sender.clone(),
                state
                    .user_settings
                    .get(&msg.author.id)
                    .cloned()
                    .unwrap_or_default(),
            )
        };

        if let Err(err) = sender
            .send(SpeechRequest {
                text: msg.content.clone(),
                settings,
            })
            .await
        {
            eprintln!("Failed to queue message for playback: {err}");
            return;
        }
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        _old: Option<serenity::all::VoiceState>,
        new: serenity::all::VoiceState,
    ) {
        let Some(guild_id) = new.guild_id else {
            return;
        };

        let state = bot_state(&ctx).await;

        if !state.read().await.playback.contains_key(&guild_id) {
            return;
        }

        if !bot_is_alone(&ctx, guild_id) {
            return;
        }

        if let Err(err) = commands::leave::leave_guild(&ctx, guild_id).await {
            eprintln!("Failed to leave empty voice channel: {err:#}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            match command.data.name.as_str() {
                "ping" => {
                    if let Err(err) = commands::ping::ping_command(&ctx, &command).await {
                        eprintln!("Failed to execute /ping: {err}")
                    }
                }
                "join" => {
                    if let Err(err) = commands::join::join_command(&ctx, &command).await {
                        eprintln!("Failed to execute /join: {err}")
                    }
                }
                "leave" => {
                    if let Err(err) = commands::leave::leave_command(&ctx, &command).await {
                        eprintln!("Failed to execute /leave: {err}")
                    }
                }
                "speed" => {
                    if let Err(err) = commands::settings::speed_command(&ctx, &command).await {
                        eprintln!("Failed to execute /speed: {err}")
                    }
                }
                "speaker" => {
                    if let Err(err) = commands::settings::speaker_command(&ctx, &command).await {
                        eprintln!("Failed to execute /speaker: {err}")
                    }
                }
                "skip" => {
                    if let Err(err) = commands::skip::skip_command(&ctx, &command).await {
                        eprintln!("Failed to execute /skip: {err}")
                    }
                }
                _ => {}
            }
        }
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let voicevox = voicevox::Voicevox::from_env().expect("Invalid VOICEVOX configuration");

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .register_songbird()
        .await
        .expect("Err creating client");

    {
        let mut data = client.data.write().await;
        data.insert::<BotStateKey>(Arc::new(RwLock::new(BotState {
            playback: HashMap::new(),
            voicevox,
            synthesis_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_SYNTHESIS)),
            user_settings: HashMap::new(),
        })));
    }

    let data = client.data.clone();
    let shard_manager = client.shard_manager.clone();
    tokio::select! {
        result = client.start() => {
            if let Err(err) = result {
                eprintln!("Client error: {err:?}");
            }
        }
        result = tokio::signal::ctrl_c() => {
            if let Err(err) = result {
                eprintln!("Failed to listen for Ctrl+C: {err}");
            }

            println!("Ctrl+C received; leaving voice channels...");
            commands::leave::leave_all(&data).await;
            shard_manager.shutdown_all().await;
        }
    }
}
