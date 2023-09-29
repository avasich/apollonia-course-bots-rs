use std::{
    error::Error,
    fmt::{Display, Formatter},
};

#[derive(Debug)]
pub struct BotError<E: Error> {
    bot_id: String,
    inner: E,
}

impl<E: Error> BotError<E> {
    pub fn new(bot_id: impl Into<String>, error: E) -> Self {
        Self {
            bot_id: bot_id.into(),
            inner: error,
        }
    }
}

impl<E: Error, S: Into<String>> From<(S, E)> for BotError<E> {
    fn from((bot_id, error): (S, E)) -> Self {
        BotError::new(bot_id, error)
    }
}

impl<E: Error> Display for BotError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "bot {} error: {}", self.bot_id, self.inner)
    }
}

impl<E: Error> Error for BotError<E> {}

#[derive(Debug)]
pub enum AddBotError {
    IdParse(String),
    UrlParse(BotError<url::ParseError>),
    Listener(BotError<teloxide::RequestError>),
}

impl Display for AddBotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use AddBotError::*;
        match self {
            IdParse(token) => {
                write!(f, "failed to parse bot id in token: {token}")
            }
            UrlParse(e) => Display::fmt(e, f),
            Listener(e) => Display::fmt(e, f),
        }
    }
}

impl Error for AddBotError {}
