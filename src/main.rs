use std::{collections::HashMap, env, sync::Arc};

use serenity::{
    all::{ChannelId, EventHandler, GatewayIntents, GuildId, Interaction, Message, Ready},
    async_trait,
    prelude::*,
};
use songbird::SerenityInit;

const MY_SERVER: u64 = 1319360646994727003;

mod commands;

pub struct BotState {
    pub text_channels: HashMap<GuildId, ChannelId>,
}

pub struct BotStateKey;

impl TypeMapKey for BotStateKey {
    type Value = Arc<RwLock<BotState>>;
}

struct Handler;

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
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let Some(guild_id) = msg.guild_id else {
            return;
        };

        let state = {
            let data = ctx.data.read().await;
            data.get::<BotStateKey>().unwrap().clone()
        };

        if Some(msg.channel_id) != state.read().await.text_channels.get(&guild_id).copied() {
            return;
        }

        let manager = songbird::get(&ctx)
            .await
            .expect("Songbird is not registered");

        let Some(call) = manager.get(guild_id) else {
            return;
        };

        println!("{}", msg.content);
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
                _ => {}
            }
        }
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

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
            text_channels: HashMap::new(),
        })));
    }

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
