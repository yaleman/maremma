//! DB Actor communicates with the rest of the system to broker requests to the database.

use std::sync::Arc;

use crate::errors::Error;
use chrono::{DateTime, Utc};
use croner::Cron;
use rand::seq::IteratorRandom;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, ModelTrait,
    QueryFilter,
};
use tokio::sync::mpsc::Receiver;
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, error, info};
use uuid::Uuid;

use super::{entities, get_next_service_check, CheckResult, Service, ServiceStatus};

#[derive(Debug)]
pub enum DbActorMessage {
    NextServiceCheck(
        oneshot::Sender<Option<(entities::service_check::Model, entities::service::Model)>>,
    ),
    GetService(Uuid, oneshot::Sender<Option<entities::service::Model>>),
    SetStatus {
        service_check_id: Uuid,
        status: ServiceStatus,
        sender: oneshot::Sender<()>,
    },
    GetRunnableCheck {
        service_check: entities::service_check::Model,
        service: entities::service::Model,
        sender: oneshot::Sender<(entities::host::Model, Service)>,
    },
    /// Sets the value in the service and also stores the service_check_history record
    SetCheckResult {
        service_check: entities::service_check::Model,
        service: entities::service::Model,
        last_check: DateTime<Utc>,
        result: CheckResult,
        jitter: u32,
    },
    Shutdown,
}

pub struct DbActor {
    database: Arc<RwLock<DatabaseConnection>>,
    receiver: Receiver<DbActorMessage>,
}

impl DbActor {
    pub fn new(
        database: Arc<RwLock<DatabaseConnection>>,
        receiver: Receiver<DbActorMessage>,
    ) -> Self {
        Self { database, receiver }
    }

    /// Keeps the actor running in a loop until we get an active shutdown signal
    pub async fn run_actor(&mut self) -> Result<(), Error> {
        loop {
            match self.run().await {
                Ok(_) => {
                    info!("DbActor run completed");
                    break;
                }
                Err(err) => error!("DbActor run error: {:?}", err),
            }
        }
        Ok(())
    }

    /// handles the messages
    pub async fn run(&mut self) -> Result<(), Error> {
        while let Some(message) = self.receiver.recv().await {
            match message {
                DbActorMessage::Shutdown => {
                    info!("DbActor received shutdown message");
                    break;
                }
                DbActorMessage::GetService(service_id, sender) => {
                    let service = entities::service::Entity::find()
                        .filter(entities::service::Column::Id.eq(service_id))
                        .one(&*self.database.write().await)
                        .await
                        .map_err(Error::from)?;

                    if let Err(err) = sender.send(service) {
                        error!("Failed to send get_service response: {:?}", err);
                    }
                }
                DbActorMessage::SetStatus {
                    service_check_id,
                    status,
                    sender,
                } => {
                    if let Some(service_check) = entities::service_check::Entity::find()
                        .filter(entities::service_check::Column::Id.eq(service_check_id))
                        .one(&*self.database.write().await)
                        .await
                        .map_err(Error::from)?
                    {
                        let mut service_check = service_check.into_active_model();
                        service_check.status.set_if_not_equals(status);
                        service_check.update(&*self.database.write().await).await?;
                        if let Err(err) = sender.send(()) {
                            error!(
                                "Failed to send set_status response after setting status {:?}",
                                err
                            );
                        }
                    };
                }
                DbActorMessage::SetCheckResult {
                    service_check,
                    service,
                    last_check,
                    result,
                    jitter,
                } => {
                    entities::service_check_history::Model::from_service_check_result(
                        service_check.id,
                        &result,
                    )
                    .into_active_model()
                    .insert(&*self.database.write().await)
                    .await?;

                    let mut model = service_check.into_active_model();
                    model.last_check.set_if_not_equals(last_check);
                    model.status.set_if_not_equals(result.status);

                    // get a number between 0 and jitter
                    let jitter: i64 =
                        (0..jitter).choose(&mut rand::thread_rng()).unwrap_or(0) as i64;

                    let next_check = Cron::new(&service.cron_schedule)
                        .parse()?
                        .find_next_occurrence(&chrono::Utc::now(), false)?
                        + chrono::Duration::seconds(jitter);
                    model.next_check.set_if_not_equals(next_check);

                    if model.is_changed() {
                        debug!("Saving {:?}", model);
                        model
                            .save(&*self.database.write().await)
                            .await
                            .map_err(|err| {
                                error!("{} error saving {:?}", service.id.hyphenated(), err);
                                Error::from(err)
                            })?;
                    } else {
                        debug!("set_last_check with no change? {:?}", model);
                    }
                }
                DbActorMessage::NextServiceCheck(tx) => {
                    let next_service = get_next_service_check(&*self.database.read().await).await?;

                    match next_service {
                        None => {
                            if let Err(err) = tx.send(None) {
                                error!(
                                    "Failed to send empty get_next_service_check response: {:?}",
                                    err
                                );
                            }
                        }
                        Some((service_check, service)) => {
                            // set the service_check to running
                            service_check
                                .set_status(ServiceStatus::Checking, self.database.clone())
                                .await?;

                            if let Err(err) = tx.send(Some((service_check.clone(), service))) {
                                error!("Failed to send next service check response: {:?}", err);
                                service_check
                                    .set_status(ServiceStatus::Pending, self.database.clone())
                                    .await?;
                            }
                        }
                    }
                }
                DbActorMessage::GetRunnableCheck {
                    service_check,
                    service,
                    sender,
                } => {
                    use crate::services::Service;
                    let service = match Service::try_from_service_model(
                        &service,
                        &*self.database.write().await,
                    )
                    .await
                    {
                        Ok(check) => check,
                        Err(err) => {
                            error!(
                                "Failed to convert service check {} to service: {:?}",
                                service_check.id, err
                            );
                            return Err(Error::Generic(format!(
                                "Failed to convert service check {} to service: {:?}",
                                service_check.id, err
                            )));
                        }
                    };

                    let host: entities::host::Model = match service_check
                        .find_related(entities::host::Entity)
                        .one(&*self.database.write().await)
                        .await?
                    {
                        Some(host) => {
                            debug!(
                                "Found host: {} for service_check={}",
                                host.name,
                                service_check.id.hyphenated()
                            );
                            host
                        }
                        None => {
                            error!(
                                "Failed to get host for service check: {:?}",
                                service_check.id
                            );
                            return Err(Error::HostNotFound(service_check.host_id));
                        }
                    };
                    if let Err(err) = sender.send((host, service)) {
                        error!("Failed to send runnable check response: {:?}", err);
                    };
                }
            }
        }

        Ok(())
    }
}
