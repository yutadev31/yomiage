use serenity::all::{CommandInteraction, Context, CreateCommand, EditInteractionResponse};

use crate::BotStateKey;

pub fn register() -> CreateCommand {
    CreateCommand::new("join").description("Replies with Pong!")
}

pub async fn join_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
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
                    .content("ボイスチャンネルに接続した状態で実行してください。"),
            )
            .await?;

        return Ok(());
    };

    let state = {
        let data = ctx.data.read().await;
        data.get::<BotStateKey>().unwrap().clone()
    };

    let voicevox = state.read().await.voicevox.clone();
    if let Err(err) = voicevox.check_connection().await {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(format!(
                    "VOICEVOX Engineに接続できません。起動後にもう一度実行してください。\n`{err:#}`"
                )),
            )
            .await?;
        return Ok(());
    }

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird is not registered");

    manager.join(guild_id, channel_id).await?;

    let state = {
        let data = ctx.data.read().await;
        data.get::<BotStateKey>().unwrap().clone()
    };

    state
        .write()
        .await
        .text_channels
        .insert(guild_id, command.channel_id);

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("接続しました。"),
        )
        .await?;

    Ok(())
}
