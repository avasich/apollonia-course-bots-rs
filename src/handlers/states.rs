use teloxide::{
    dispatching::{dialogue, dialogue::InMemStorage, UpdateFilterExt, UpdateHandler},
    prelude::*,
    types::Update,
    utils::command::BotCommands,
    Bot,
};

use super::{admin, reader};

pub type HandlerError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type HandlerResult = Result<(), HandlerError>;
pub type MyDialogue = Dialogue<State, InMemStorage<State>>;

#[derive(Clone, Default)]
pub enum State {
    #[default]
    Start,
    Admin(admin::AdminState),
    Reader(reader::ReaderState),
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum Command {
    /// Display this text.
    Help,
    /// Start bot.
    Start,
    /// Go to admin panel.
    Admin,
}

async fn start(bot: Bot, msg: Message, dialogue: MyDialogue) -> HandlerResult {
    let text = msg.text().unwrap_or("");

    let id = bot.get_me().await?.id;
    let token = bot.token();

    bot.send_message(
        msg.chat.id,
        format!("start({text})\nbot id: {id}\nbot token: {token}"),
    )
    .await?;
    Ok(())
}

async fn command(bot: Bot, msg: Message, dialogue: MyDialogue, cmd: Command) -> HandlerResult {
    match cmd {
        Command::Help => bot.send_message(msg.chat.id, "no help for you"),
        Command::Start => bot.send_message(msg.chat.id, "starting, starting"),
        Command::Admin => bot.send_message(msg.chat.id, "you are not an admin"),
    }
    .await?;

    Ok(())
}

pub fn schema() -> UpdateHandler<HandlerError> {
    use dptree::case;

    let start_handler = dptree::endpoint(start);
    let command_handler = teloxide::filter_command::<Command, _>().endpoint(command);

    let message_handler = Update::filter_message()
        .branch(command_handler)
        .branch(case![State::Start].chain(start_handler))
        .branch(case![State::Admin(state)].chain(admin::schema()))
        .branch(case![State::Reader(state)].chain(reader::schema()));

    dialogue::enter::<Update, InMemStorage<State>, State, _>().branch(message_handler)
}
