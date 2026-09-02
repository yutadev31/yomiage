use serenity::all::{CommandInteraction, Context, CreateCommand, EditInteractionResponse};

use crate::BotStateKey;

pub fn register() -> CreateCommand {
    CreateCommand::new("leave").description("Replies with Pong!")
}

pub async fn leave_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(guild_id) = command.guild_id else {
        return Ok(());
    };

    let channel_id = {
        let guild = ctx.cache.guild(guild_id).unwrap();

        guild
            .voice_states
            .get(&command.user.id)
            .and_then(|state| state.channel_id)
    };

    let Some(channel_id) = channel_id else {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .content("ボイスチャンネルに接続して実行してください。"),
            )
            .await?;

        return Ok(());
    };

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird is not registered");

    manager.leave(guild_id).await?;

    let state = {
        let data = ctx.data.read().await;
        data.get::<BotStateKey>().unwrap().clone()
    };

    state
        .write()
        .await
        .text_channels
        .insert(guild_id, channel_id);

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("切断しました。"),
        )
        .await?;

    Ok(())
}
