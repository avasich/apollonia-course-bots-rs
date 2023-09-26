use std::{fmt::Debug, net::SocketAddr, vec};

use axum::Router;
use dotenv::dotenv;
use futures::future::BoxFuture;
use teloxide::{
    dispatching::{dialogue::InMemStorage, DefaultKey, UpdateHandler},
    prelude::*,
    stop::StopToken,
    update_listeners::{webhooks, UpdateListener},
    Bot,
};
use tokio::task::JoinSet;

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
            .await;
    }

    let (_, bots_handles) = builder.build();
    futures::future::join_all(bots_handles).await;
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
    stop_flags: JoinSet<()>,
    bots_listeners: Vec<BoxFuture<'static, ()>>,
}

impl BotServerBuilder {
    fn new(host: String, addr: SocketAddr) -> Self {
        Self {
            host,
            addr,
            router: Router::new(),
            stop_tokens: vec![],
            stop_flags: JoinSet::new(),
            bots_listeners: vec![],
        }
    }

    async fn add_bot<Err>(
        &mut self,
        token: String,
        make_dispatcher: impl FnOnce(Bot) -> Dispatcher<Bot, Err, DefaultKey> + Send + 'static,
    ) -> Result<(), <Bot as Requester>::Err>
    where
        Err: Debug + Send + Sync + 'static,
    {
        let bot = Bot::new(&token);
        let url = format!("{}/{}", self.host, token).parse().unwrap();

        let (mut listener, stop_flag, router) =
            webhooks::axum_to_router(bot.clone(), webhooks::Options::new(self.addr, url)).await?;

        let old_router = std::mem::take(&mut self.router);
        self.router = old_router.merge(router);

        let stop_token = listener.stop_token();
        self.stop_tokens.push(stop_token);
        self.stop_flags.spawn(stop_flag);

        let dispatcher_handle = async move {
            make_dispatcher(bot.clone())
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

    fn build(
        self,
    ) -> (
        tokio::task::JoinHandle<()>,
        Vec<tokio::task::JoinHandle<()>>,
    ) {
        let Self {
            stop_tokens,
            mut stop_flags,
            bots_listeners,
            router,
            addr,
            ..
        } = self;

        let stop_flag = async move { if (stop_flags.join_next().await).is_some() {} };

        let server_handle = tokio::spawn(async move {
            axum::Server::bind(&addr)
                .serve(router.into_make_service())
                .with_graceful_shutdown(stop_flag)
                .await
                .map_err(|err| {
                    stop_tokens.iter().for_each(|t| t.stop());
                    err
                })
                .expect("Axum server error");
        });

        let bots_handles: Vec<_> = bots_listeners.into_iter().map(tokio::spawn).collect();

        (server_handle, bots_handles)
    }
}
