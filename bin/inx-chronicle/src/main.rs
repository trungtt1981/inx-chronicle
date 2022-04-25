// Copyright 2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! TODO

/// Module containing the API.
#[cfg(feature = "api")]
pub mod api;
mod broker;
mod cli;
mod config;
#[cfg(feature = "stardust")]
mod inx_listener;

use std::{error::Error, ops::Deref, time::Duration};

#[cfg(feature = "api")]
use api::ApiWorker;
use async_trait::async_trait;
use broker::{Broker, BrokerError};
#[cfg(feature = "stardust")]
use chronicle::{
    db::MongoDbError,
    inx::InxError,
    runtime::{
        actor::{
            addr::{Addr, SendError},
            context::ActorContext,
            error::ActorError,
            event::HandleEvent,
            report::Report,
            Actor,
        },
        error::RuntimeError,
        scope::RuntimeScope,
        Runtime,
    },
};
use clap::Parser;
use config::{Config, ConfigError};
#[cfg(feature = "stardust")]
use inx_listener::{InxListener, InxListenerError};
use mongodb::error::ErrorKind;
use thiserror::Error;

use self::cli::CliArgs;

#[derive(Debug, Error)]
pub enum LauncherError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    MongoDb(#[from] MongoDbError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Send(#[from] SendError),
}

#[derive(Debug)]
/// Supervisor actor
pub struct Launcher {
    inx_connection_retry_interval: Duration,
}

#[async_trait]
impl Actor for Launcher {
    type State = (Config, Addr<Broker>);
    type Error = LauncherError;

    async fn init(&mut self, cx: &mut ActorContext<Self>) -> Result<Self::State, Self::Error> {
        let cli_args = CliArgs::parse();
        let mut config = match &cli_args.config {
            Some(path) => config::Config::from_file(path)?,
            None => {
                if let Ok(path) = std::env::var("CONFIG_PATH") {
                    config::Config::from_file(path)?
                } else {
                    Config::default()
                }
            }
        };
        config.apply_cli_args(cli_args);

        let db = config.mongodb.clone().build().await?;
        let broker_addr = cx.spawn_actor_supervised(Broker::new(db.clone())).await;
        #[cfg(feature = "stardust")]
        cx.spawn_actor_supervised(InxListener::new(config.inx.clone(), broker_addr.clone()))
            .await;
        #[cfg(feature = "api")]
        cx.spawn_actor_supervised(ApiWorker::new(db)).await;
        Ok((config, broker_addr))
    }
}

#[async_trait]
impl HandleEvent<Report<Broker>> for Launcher {
    async fn handle_event(
        &mut self,
        cx: &mut ActorContext<Self>,
        event: Report<Broker>,
        (config, broker_addr): &mut Self::State,
    ) -> Result<(), Self::Error> {
        match event {
            Ok(_) => {
                cx.shutdown();
            }
            Err(e) => match e.error {
                ActorError::Result(e) => match e.deref() {
                    BrokerError::RuntimeError(_) => {
                        cx.shutdown();
                    }
                    BrokerError::MongoDbError(e) => match e {
                        chronicle::db::MongoDbError::DatabaseError(e) => match e.kind.as_ref() {
                            // Only a few possible errors we could potentially recover from
                            ErrorKind::Io(_) | ErrorKind::ServerSelection { message: _, .. } => {
                                let db = config.mongodb.clone().build().await?;
                                let handle = cx.spawn_actor_supervised(Broker::new(db)).await;
                                *broker_addr = handle;
                            }
                            _ => {
                                cx.shutdown();
                            }
                        },
                        other => {
                            log::warn!("Unhandled MongoDB error: {}", other);
                            cx.shutdown();
                        }
                    },
                },
                ActorError::Panic | ActorError::Aborted => {
                    cx.shutdown();
                }
            },
        }
        Ok(())
    }
}

#[cfg(feature = "stardust")]
#[async_trait]
impl HandleEvent<Report<InxListener>> for Launcher {
    async fn handle_event(
        &mut self,
        cx: &mut ActorContext<Self>,
        event: Report<InxListener>,
        (config, broker_addr): &mut Self::State,
    ) -> Result<(), Self::Error> {
        match &event {
            Ok(_) => {
                cx.shutdown();
            }
            Err(e) => match &e.error {
                ActorError::Result(e) => match e.deref() {
                    InxListenerError::Inx(e) => match e {
                        InxError::ConnectionError(_) => {
                            let wait_interval = self.inx_connection_retry_interval;
                            log::info!("Retrying INX connection in {} seconds.", wait_interval.as_secs_f32());
                            tokio::time::sleep(wait_interval).await;
                            cx.spawn_actor_supervised(InxListener::new(config.inx.clone(), broker_addr.clone()))
                                .await;
                        }
                        InxError::InvalidAddress(_) => {
                            cx.shutdown();
                        }
                        InxError::ParsingAddressFailed(_) => {
                            cx.shutdown();
                        }
                        // TODO: This is stupid, but we can't use the ErrorKind enum so :shrug:
                        InxError::TransportFailed(e) => match e.to_string().as_ref() {
                            "transport error" => {
                                cx.spawn_actor_supervised(InxListener::new(config.inx.clone(), broker_addr.clone()))
                                    .await;
                            }
                            _ => {
                                cx.shutdown();
                            }
                        },
                    },
                    InxListenerError::Read(_) => {
                        cx.shutdown();
                    }
                    InxListenerError::Runtime(_) => {
                        cx.shutdown();
                    }
                    InxListenerError::MissingBroker => {
                        // If the handle is still closed, push this to the back of the event queue.
                        // Hopefully when it is processed again the handle will have been recreated.
                        if broker_addr.is_closed() {
                            cx.delay(event, None)?;
                        } else {
                            cx.spawn_actor_supervised(InxListener::new(config.inx.clone(), broker_addr.clone()))
                                .await;
                        }
                    }
                },
                ActorError::Panic | ActorError::Aborted => {
                    cx.shutdown();
                }
            },
        }
        Ok(())
    }
}

#[cfg(feature = "api")]
#[async_trait]
impl HandleEvent<Report<ApiWorker>> for Launcher {
    async fn handle_event(
        &mut self,
        cx: &mut ActorContext<Self>,
        event: Report<ApiWorker>,
        (config, _): &mut Self::State,
    ) -> Result<(), Self::Error> {
        match event {
            Ok(_) => {
                cx.shutdown();
            }
            Err(e) => match e.error {
                ActorError::Result(_) => {
                    let db = config.mongodb.clone().build().await?;
                    cx.spawn_actor_supervised(ApiWorker::new(db)).await;
                }
                ActorError::Panic | ActorError::Aborted => {
                    cx.shutdown();
                }
            },
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    std::panic::set_hook(Box::new(|p| {
        log::error!("{}", p);
    }));

    if let Err(e) = Runtime::launch(startup).await {
        log::error!("{}", e);
    }
}

async fn startup(scope: &mut RuntimeScope) -> Result<(), Box<dyn Error + Send + Sync>> {
    let launcher = Launcher {
        inx_connection_retry_interval: std::time::Duration::from_secs(5),
    };

    let launcher_addr = scope.spawn_actor(launcher).await;

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        launcher_addr.shutdown();
    });

    Ok(())
}