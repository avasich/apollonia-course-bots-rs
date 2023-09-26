use teloxide::{dispatching::UpdateHandler, prelude::*};

use super::states::{HandlerError, HandlerResult};

#[derive(Clone, Default)]
pub enum ReaderState {
    #[default]
    Start,
}

async fn reader(bot: Bot, msg: Message) -> HandlerResult {
    bot.send_message(msg.chat.id, "reader").await?;
    Ok(())
}

pub fn schema() -> UpdateHandler<HandlerError> {
    return dptree::endpoint(reader);
}
