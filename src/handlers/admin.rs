use super::states::{HandlerError, HandlerResult};
use teloxide::{dispatching::UpdateHandler, dptree, prelude::*, types::Message, Bot};

#[derive(Clone, Default)]
pub enum AdminState {
    #[default]
    Start,
}

async fn admin(bot: Bot, msg: Message) -> HandlerResult {
    bot.send_message(msg.chat.id, "admin default").await?;
    Ok(())
}

pub fn schema() -> UpdateHandler<HandlerError> {
    return dptree::endpoint(admin);
}
