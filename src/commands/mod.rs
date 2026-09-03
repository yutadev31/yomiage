use serenity::all::CreateCommand;

pub mod join;
pub mod leave;
pub mod ping;
pub mod settings;
pub mod skip;

/// All slash commands exposed by the bot.
pub fn register_all() -> Vec<CreateCommand> {
    vec![
        ping::register(),
        join::register(),
        leave::register(),
        settings::register_speed(),
        settings::register_speaker(),
        skip::register(),
    ]
}
