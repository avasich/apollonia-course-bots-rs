use axum::Router;
use dotenv::dotenv;
use futures::future::BoxFuture;
use std::{fmt::Debug, hash::Hash, net::SocketAddr, vec};
use teloxide::{
    dispatching::{dialogue::InMemStorage, DefaultKey, UpdateHandler},
    prelude::*,
    stop::StopToken,
    update_listeners::{webhooks, UpdateListener},
    Bot,
};
use tokio::task::{JoinHandle, JoinSet};

use crate::handlers::{schema, State};

mod handlers;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let tokens = std::env::var("BOT_TOKENS").expect("BOT_TOKENS is not set");
    let host = std::env::var("BOT_HOST").expect("BOT_HOST is not set");
    let port: u16 = std::env::var("PORT")
        .expect("PORT is not set")
        .parse()
        .expect("cannot parse PORT");
    let addr = ([127, 0, 0, 1], port).into();

    let mut builder = BotServerBuilder::new(host, addr);

    for token in tokens.split(';') {
        let _ = builder
            .add_bot(
                token.to_owned(),
                create_dispatcher(schema(), Some(dptree::deps![InMemStorage::<State>::new()])),
            )
            .await
            .map_err(|(bot_id, err)| {
                println!("bot {bot_id} not started: {err}");
            });
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
    ) -> Result<(), (String, <Bot as Requester>::Err)>
    where
        Err: Send + Sync + 'static,
        Key: Send + Hash + Eq + Clone,
    {
        let bot = Bot::new(&token);
        let bot_id = token
            .split(':')
            .next()
            .expect("wrong bot token format")
            .to_owned();
        let url = format!("{}/bot/{}", self.host, bot_id).parse().unwrap();

        let (mut listener, stop_flag, router) =
            webhooks::axum_to_router(bot.clone(), webhooks::Options::new(self.addr, url))
                .await
                .map_err(|e| (bot_id, e))?;

        self.router = router.merge(std::mem::take(&mut self.router));

        let stop_token = listener.stop_token();
        self.stop_tokens.push(stop_token);
        self.stop_flags.push(Box::pin(stop_flag));

        let dispatcher_handle = async move {
            make_dispatcher(bot)
                .dispatch_with_listener(
                    listener,
                    LoggingErrorHandler::with_custom_text(
                        "main::An error from the update listener",
                    ),
                )
                .await;
        };
        self.bots_listeners.push(Box::pin(dispatcher_handle));
        Ok(())
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
            axum::Server::bind(&addr)
                .serve(router.into_make_service())
                // .with_graceful_shutdown(stop_flag)
                .await
                .map_err(|err| {
                    stop_tokens.iter().for_each(StopToken::stop);
                    err
                })
                .expect("Axum server error");
        });

        let mut bots_tasks = JoinSet::new();
        for listener in bots_listeners {
            bots_tasks.spawn(listener);
        }

        (server_handle, bots_tasks)
    }
}
