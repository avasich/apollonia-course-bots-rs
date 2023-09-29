use std::{fmt::Debug, hash::Hash, net::SocketAddr, vec};

use axum::Router;
use futures::future::BoxFuture;
use teloxide::{
    dispatching::{dialogue::InMemStorage, DefaultKey, UpdateHandler},
    prelude::*,
    stop::StopToken,
    update_listeners::{webhooks, UpdateListener},
    Bot,
};
use tokio::task::{JoinHandle, JoinSet};

use crate::{
    error::AddBotError,
    handlers::{schema, State},
};

mod error;
mod handlers;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    pretty_env_logger::init();

    let tokens = std::env::var("BOT_TOKENS")
        .map_err(|e| log::error!("BOT_TOKENS: {e}"))
        .unwrap();
    let host = std::env::var("BOT_HOST")
        .map_err(|e| log::error!("BOT_TOKENS: {e}"))
        .unwrap();
    let port: u16 = std::env::var("PORT")
        .map_err(|e| log::error!("PORT: {e}"))
        .unwrap()
        .parse()
        .map_err(|e| log::error!("PORT: {e}"))
        .unwrap();
    let addr = ([127, 0, 0, 1], port).into();

    let mut builder = BotServerBuilder::new(host, addr);

    for token in tokens.split(';') {
        let _ = builder
            .add_bot(
                token.to_owned(),
                create_dispatcher(schema(), Some(dptree::deps![InMemStorage::<State>::new()])),
            )
            .await
            .map(|id| log::info!("webhook for bot {id} set up"))
            .map_err(|e| log::error!("bot is not started: {e}"));
    }

    let (_, mut bots_handles) = builder.build();
    while (bots_handles.join_next().await).is_some() {}
}

fn create_dispatcher<Err>(
    schema: UpdateHandler<Err>,
    dependencies: Option<DependencyMap>,
) -> impl FnOnce(Bot) -> Dispatcher<Bot, Err, DefaultKey>
where
    Err: Debug + Send + Sync + 'static,
{
    |bot: Bot| {
        Dispatcher::builder(bot, schema)
            .dependencies(dependencies.unwrap_or(dptree::deps![]))
            .build()
    }
}

struct BotServerBuilder {
    host: String,
    addr: SocketAddr,
    router: Router,
    stop_tokens: Vec<StopToken>,
    stop_flags: Vec<BoxFuture<'static, ()>>,
    bots_listeners: Vec<BoxFuture<'static, ()>>,
}

impl BotServerBuilder {
    fn new(host: String, addr: SocketAddr) -> Self {
        Self {
            host,
            addr,
            router: Router::new(),
            stop_tokens: vec![],
            stop_flags: vec![],
            bots_listeners: vec![],
        }
    }

    async fn add_bot<Err, Key>(
        &mut self,
        token: String,
        make_dispatcher: impl FnOnce(Bot) -> Dispatcher<Bot, Err, Key> + Send + 'static,
    ) -> Result<String, AddBotError>
    where
        Err: Send + Sync + 'static,
        Key: Send + Hash + Eq + Clone,
    {
        let bot = Bot::new(&token);

        let bot_id = token
            .split(':')
            .next()
            .ok_or_else(|| AddBotError::IdParse(token.clone()))?;

        let url = format!("{}/bot/{}", self.host, bot_id)
            .parse()
            .map_err(|e| AddBotError::UrlParse((bot_id, e).into()))?;

        let (mut listener, stop_flag, router) =
            webhooks::axum_to_router(bot.clone(), webhooks::Options::new(self.addr, url))
                .await
                .map_err(|e| AddBotError::Listener((bot_id, e).into()))?;

        self.router = router.merge(std::mem::take(&mut self.router));

        let stop_token = listener.stop_token();
        self.stop_tokens.push(stop_token);
        self.stop_flags.push(Box::pin(stop_flag));

        let bot_id_owned = bot_id.to_string();
        let dispatcher_handle = async move {
            log::info!("listening webhook for bot {bot_id_owned}");
            make_dispatcher(bot)
                .dispatch_with_listener(
                    listener,
                    LoggingErrorHandler::with_custom_text(format!(
                        "bot {} listener error",
                        bot_id_owned
                    )),
                )
                .await;
        };
        self.bots_listeners.push(Box::pin(dispatcher_handle));
        Ok(bot_id.into())
    }

    fn build(self) -> (JoinHandle<()>, JoinSet<()>) {
        let Self {
            stop_tokens,
            bots_listeners,
            router,
            addr,
            ..
        } = self;

        let server_handle = tokio::spawn(async move {
            let _ = axum::Server::bind(&addr)
                .serve(router.into_make_service())
                // .with_graceful_shutdown(stop_flag)
                .await
                .map(|_| log::info!("axum server started"))
                .map_err(|e| {
                    log::error!("axum server error: {e}, stopping bots");
                    stop_tokens.iter().for_each(StopToken::stop);
                });
        });

        let mut bots_tasks = JoinSet::new();
        for listener in bots_listeners {
            bots_tasks.spawn(listener);
        }

        (server_handle, bots_tasks)
    }
}
