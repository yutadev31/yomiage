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

    let state = {
        let data = ctx.data.read().await;
        data.get::<BotStateKey>().unwrap().clone()
    };

    if state.read().await.text_channels.get(&guild_id).is_none() {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("ボイスチャンネルに接続していません。"),
            )
            .await?;

        return Ok(());
    }

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird is not registered");

    manager.leave(guild_id).await?;

    state.write().await.text_channels.remove(&guild_id);

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("切断しました。"),
        )
        .await?;

    Ok(())
}
