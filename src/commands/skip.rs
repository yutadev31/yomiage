use serenity::all::{CommandInteraction, Context, CreateCommand, EditInteractionResponse};

use crate::bot_state;

pub fn register() -> CreateCommand {
    CreateCommand::new("skip").description("現在読み上げ中のメッセージをスキップします")
}

pub async fn skip_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(guild_id) = command.guild_id else {
        return Ok(());
    };

    let state = bot_state(ctx).await;
    let skip_sender = {
        let state = state.read().await;
        let Some(playback) = state.playback.get(&guild_id) else {
            respond(ctx, command, "ボイスチャンネルに接続していません。").await?;
            return Ok(());
        };
        if playback.text_channel_id != command.channel_id {
            respond(
                ctx,
                command,
                "読み上げ対象のテキストチャンネルで実行してください。",
            )
            .await?;
            return Ok(());
        }
        playback.skip_sender.clone()
    };

    skip_sender.send_modify(|version| *version += 1);

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird is not registered");
    if let Some(call) = manager.get(guild_id) {
        call.lock().await.queue().stop();
    }

    respond(
        ctx,
        command,
        "現在読み上げ中のメッセージをスキップしました。",
    )
    .await
}

async fn respond(
    ctx: &Context,
    command: &CommandInteraction,
    content: impl Into<String>,
) -> anyhow::Result<()> {
    command
        .edit_response(&ctx.http, EditInteractionResponse::new().content(content))
        .await?;
    Ok(())
}
