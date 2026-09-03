use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, Context, CreateCommand,
    CreateCommandOption, EditInteractionResponse,
};

use crate::bot_state;

const MIN_SPEED: f64 = 0.5;
const MAX_SPEED: f64 = 2.0;

pub fn register_speed() -> CreateCommand {
    CreateCommand::new("speed")
        .description("自分の読み上げ速度を設定します")
        .add_option(
            CreateCommandOption::new(CommandOptionType::Number, "value", "速度（0.5〜2.0）")
                .required(true)
                .min_number_value(MIN_SPEED)
                .max_number_value(MAX_SPEED),
        )
}

pub fn register_speaker() -> CreateCommand {
    CreateCommand::new("speaker")
        .description("自分のVOICEVOX話者IDを設定します")
        .add_option(
            CreateCommandOption::new(CommandOptionType::Integer, "id", "VOICEVOX話者ID")
                .required(true),
        )
}

pub async fn speed_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(CommandDataOptionValue::Number(speed)) =
        command.data.options.first().map(|option| &option.value)
    else {
        respond(ctx, command, "速度を指定してください。例: `/speed 1.2`").await?;
        return Ok(());
    };

    let state = bot_state(ctx).await;
    state
        .write()
        .await
        .user_settings
        .entry(command.user.id)
        .or_default()
        .speed = Some(*speed);
    respond(
        ctx,
        command,
        format!("読み上げ速度を `{speed:.2}` に設定しました。"),
    )
    .await
}

pub async fn speaker_command(ctx: &Context, command: &CommandInteraction) -> anyhow::Result<()> {
    command.defer(&ctx.http).await?;

    let Some(CommandDataOptionValue::Integer(speaker)) =
        command.data.options.first().map(|option| &option.value)
    else {
        respond(ctx, command, "話者IDを指定してください。例: `/speaker 3`").await?;
        return Ok(());
    };
    let Ok(speaker) = u32::try_from(*speaker) else {
        respond(ctx, command, "話者IDは0以上の整数で指定してください。").await?;
        return Ok(());
    };

    let state = bot_state(ctx).await;
    state
        .write()
        .await
        .user_settings
        .entry(command.user.id)
        .or_default()
        .speaker = Some(speaker);
    respond(
        ctx,
        command,
        format!("VOICEVOX話者IDを `{speaker}` に設定しました。"),
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
