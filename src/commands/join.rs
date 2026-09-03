use serenity::all::{
    ChannelId, CommandInteraction, Context, CreateCommand, EditInteractionResponse, GuildId, UserId,
};

use crate::BotStateKey;

pub fn register() -> CreateCommand {
    CreateCommand::new("join").description("Replies with Pong!")
}

pub async fn join_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(guild_id) = command.guild_id else {
        return Ok(());
    };

    if let Err(err) = join(ctx, guild_id, command.user.id, command.channel_id).await {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(err.to_string()),
            )
            .await?;
        return Ok(());
    }

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("接続しました。"),
        )
        .await?;

    Ok(())
}

/// Joins the requesting user's voice channel and remembers the text channel to read.
pub async fn join(
    ctx: &Context,
    guild_id: GuildId,
    user_id: UserId,
    text_channel_id: ChannelId,
) -> Result<(), String> {
    let channel_id = ctx
        .cache
        .guild(guild_id)
        .and_then(|guild| {
            guild
                .voice_states
                .get(&user_id)
                .and_then(|state| state.channel_id)
        })
        .ok_or_else(|| "ボイスチャンネルに接続した状態で実行してください。".to_owned())?;

    let state = {
        let data = ctx.data.read().await;
        data.get::<BotStateKey>().unwrap().clone()
    };

    let voicevox = state.read().await.voicevox.clone();
    if let Err(err) = voicevox.check_connection().await {
        return Err(format!(
            "VOICEVOX Engineに接続できません。起動後にもう一度実行してください。\n`{err:#}`"
        ));
    }

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird is not registered");

    manager
        .join(guild_id, channel_id)
        .await
        .map_err(|err| format!("ボイスチャンネルへの接続に失敗しました: {err:#}"))?;

    state
        .write()
        .await
        .text_channels
        .insert(guild_id, text_channel_id);

    Ok(())
}
