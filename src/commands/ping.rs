use serenity::all::{
    CommandInteraction, Context, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage,
};

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("Botの応答を確認します")
}

pub async fn ping_command(ctx: &Context, command: &CommandInteraction) -> serenity::Result<()> {
    command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("Pong!"),
            ),
        )
        .await
}
