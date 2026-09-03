use std::sync::Arc;

use serenity::{
    all::{
        ChannelId, CommandInteraction, Context, CreateCommand, EditInteractionResponse, GuildId,
        UserId,
    },
    prelude::Mutex,
};
use songbird::Call;
use tokio::{
    sync::{Semaphore, mpsc},
    task::AbortHandle,
};

use crate::{
    GuildPlayback, MESSAGE_QUEUE_CAPACITY, bot_state, chunking::split_text, voicevox::Voicevox,
};

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

    let state = bot_state(ctx).await;

    let (voicevox, synthesis_permits) = {
        let state = state.read().await;
        (state.voicevox.clone(), state.synthesis_permits.clone())
    };
    if let Err(err) = voicevox.check_connection().await {
        return Err(format!(
            "VOICEVOX Engineに接続できません。起動後にもう一度実行してください。\n`{err:#}`"
        ));
    }

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird is not registered");

    let call = manager
        .join(guild_id, channel_id)
        .await
        .map_err(|err| format!("ボイスチャンネルへの接続に失敗しました: {err:#}"))?;

    let (sender, task) = start_playback_worker(voicevox, call, synthesis_permits);

    let previous = state.write().await.playback.insert(
        guild_id,
        GuildPlayback {
            text_channel_id,
            sender,
            task,
        },
    );
    if let Some(previous) = previous {
        previous.task.abort();
    }

    Ok(())
}

fn start_playback_worker(
    voicevox: Voicevox,
    call: Arc<Mutex<Call>>,
    synthesis_permits: Arc<Semaphore>,
) -> (mpsc::Sender<String>, AbortHandle) {
    let (sender, mut receiver) = mpsc::channel::<String>(MESSAGE_QUEUE_CAPACITY);
    let task = tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            for chunk in split_text(&message) {
                if chunk.trim().is_empty() {
                    continue;
                }

                let wav = {
                    let _permit = synthesis_permits
                        .acquire()
                        .await
                        .expect("synthesis semaphore must remain open");
                    voicevox.synthesize(&chunk).await
                };
                let wav = match wav {
                    Ok(wav) => wav,
                    Err(err) => {
                        eprintln!("Failed to synthesize VOICEVOX chunk: {err:#}");
                        continue;
                    }
                };

                call.lock().await.enqueue_input(wav.into()).await;
            }
        }
    });

    (sender, task.abort_handle())
}
