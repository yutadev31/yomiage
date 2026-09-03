use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context, CreateCommand, EditInteractionResponse, GuildId},
    prelude::{RwLock, TypeMap},
};
use songbird::serenity::SongbirdKey;

use crate::bot_state;

pub fn register() -> CreateCommand {
    CreateCommand::new("leave").description("Replies with Pong!")
}

pub async fn leave_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(guild_id) = command.guild_id else {
        return Ok(());
    };

    let state = bot_state(ctx).await;

    if state.read().await.text_channels.get(&guild_id).is_none() {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("ボイスチャンネルに接続していません。"),
            )
            .await?;

        return Ok(());
    }

    leave_guild(ctx, guild_id).await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("切断しました。"),
        )
        .await?;

    Ok(())
}

/// Leaves the guild's voice channel and clears its text-channel association.
pub async fn leave_guild(ctx: &Context, guild_id: GuildId) -> anyhow::Result<()> {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird is not registered");

    manager.leave(guild_id).await?;

    let state = bot_state(ctx).await;
    state.write().await.text_channels.remove(&guild_id);

    Ok(())
}

/// Leaves every voice channel managed by the bot before the process stops.
pub async fn leave_all(data: &Arc<RwLock<TypeMap>>) {
    let manager = {
        let data = data.read().await;
        data.get::<SongbirdKey>()
            .expect("Songbird is not registered")
            .clone()
    };
    let state = {
        let data = data.read().await;
        data.get::<crate::BotStateKey>()
            .expect("Bot state is not initialized")
            .clone()
    };
    let guild_ids: Vec<_> = state.read().await.text_channels.keys().copied().collect();

    for guild_id in guild_ids {
        if let Err(err) = manager.leave(guild_id).await {
            eprintln!("Failed to leave voice channel in guild {guild_id}: {err:#}");
            continue;
        }
        state.write().await.text_channels.remove(&guild_id);
    }
}
