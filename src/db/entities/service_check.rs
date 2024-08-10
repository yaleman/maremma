use entities::service_check_history;
use sea_orm::{FromQueryResult, JoinType, QuerySelect, Set};

use crate::prelude::*;

use super::{host, host_group, host_group_members, service};

#[derive(Clone, Debug, Default, PartialEq, Eq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "service_check")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub host_id: Uuid,
    pub status: ServiceStatus,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub next_check: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl Model {
    #[instrument(skip(self, db), fields(service_check_id = self.id.hyphenated().to_string(), host_id=self.host_id.hyphenated().to_string()))]
    pub async fn set_status(
        &self,
        status: ServiceStatus,
        db: &DatabaseConnection,
    ) -> Result<(), Error> {
        let mut model = self.clone().into_active_model();
        model.status = Set(status);
        model.save(db).await.map_err(Error::from)?;
        Ok(())
    }

    #[instrument(skip(self, db), fields(service_check_id = self.id.to_string()))]
    pub async fn set_last_check(
        &self,
        service: &service::Model,
        last_check: chrono::DateTime<chrono::Utc>,
        status: ServiceStatus,
        db: &DatabaseConnection,
    ) -> Result<(), Error> {
        let mut model = self.clone().into_active_model();
        model.last_check.set_if_not_equals(last_check);
        model.status.set_if_not_equals(status);
        if model.is_changed() {
            model.save(db).await.map_err(Error::from)?;
        }
        self.set_next_check(service, db).await?;
        Ok(())
    }

    // #[instrument(skip_all, fields(service_check_id = self.id.to_string()))]
    pub async fn set_next_check(
        &self,
        service: &service::Model,
        db: &DatabaseConnection,
    ) -> Result<(), Error> {
        let mut model = self.clone().into_active_model();
        let next_check: Cron = Cron::new(&service.cron_schedule).parse()?;
        let next_check = next_check.find_next_occurrence(&chrono::Utc::now(), false)?;
        model.next_check.set_if_not_equals(next_check);
        if model.is_changed() {
            info!(
                "service_check_id={} saving next check: {}",
                self.id.hyphenated(),
                next_check.to_rfc3339()
            );
            model.save(db).await.map_err(Error::from)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Service,
    Host,
    ServiceCheckHistory,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Service => Entity::belongs_to(service::Entity)
                .from(Column::ServiceId)
                .to(service::Column::Id)
                .into(),
            Self::Host => Entity::belongs_to(host::Entity)
                .from(Column::HostId)
                .to(host::Column::Id)
                .into(),
            Self::ServiceCheckHistory => Entity::belongs_to(service_check_history::Entity)
                .from(Column::Id)
                .to(service_check_history::Column::ServiceCheckId)
                .into(),
        }
    }
}

impl Related<service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}

impl Related<host::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Host.def()
    }
}

impl Related<service_check_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ServiceCheckHistory.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

async fn update_local_services_from_db(
    db: Arc<DatabaseConnection>,
    config: Arc<Configuration>,
) -> Result<(), Error> {
    let local_host_id = match host::Entity::find()
        .filter(host::Column::Hostname.eq(crate::LOCAL_SERVICE_HOST_NAME))
        .one(db.as_ref())
        .await
        .map_err(Error::from)?
        .map(|h| h.id)
    {
        Some(val) => val,
        None => {
            host::Entity::insert(
                host::Model {
                    id: Uuid::new_v4(),
                    name: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                    hostname: crate::LOCAL_SERVICE_HOST_NAME.to_string(),
                    check: crate::host::HostCheck::None,
                }
                .into_active_model(),
            )
            .exec_with_returning(db.as_ref())
            .await?
            .id
        }
    };

    for service in config.local_services.services.clone() {
        debug!("Ensuring local service exists: {}", service);
        // can we find the service?

        let service_id = service::Entity::find()
            .filter(service::Column::Name.eq(service.as_str()))
            .one(db.as_ref())
            .await
            .map_err(Error::from)?
            .ok_or_else(|| Error::ServiceNotFoundByName(service.clone()))?
            .id;

        // if we can't find it, add it.
        if Entity::find()
            .filter(Column::HostId.eq(local_host_id))
            .filter(Column::ServiceId.eq(service_id))
            .one(db.as_ref())
            .await
            .map_err(Error::from)?
            .is_none()
        {
            debug!("Adding local service check: {}", service);
            Entity::insert(
                Model {
                    id: Uuid::new_v4(),
                    service_id,
                    host_id: local_host_id,
                    status: ServiceStatus::Unknown,
                    last_check: chrono::Utc::now(),
                    next_check: chrono::Utc::now(),
                    last_updated: chrono::Utc::now(),
                }
                .into_active_model(),
            )
            .exec(db.as_ref())
            .await
            .map_err(Error::from)?;
        };
    }

    Ok(())
}

#[async_trait]
impl MaremmaEntity for Model {
    /// This updates all the service checks. It really needs to be run after you've added all the hosts and services and host_groups!
    async fn update_db_from_config(
        db: Arc<DatabaseConnection>,
        config: Arc<Configuration>,
    ) -> Result<(), Error> {
        debug!("Starting update of service checks");
        // the easy ones are the locals.
        info!("Starting local updates...");
        update_local_services_from_db(db.clone(), config).await?;

        info!("Starting remote updates...");
        // now we're doing the other services!
        let services = service::Entity::find().all(db.as_ref()).await?;

        if services.is_empty() {
            error!("No services found, skipping service check update");
            return Ok(());
        } else {
            debug!("Found {} services", services.len());
        }

        for service in services.into_iter() {
            let service_id = service.id;

            debug!("Checking groups for service: {}", service.name);
            let host_groups: Vec<String> = match serde_json::from_value(service.host_groups) {
                Ok(host_groups) => host_groups,
                Err(err) => {
                    error!(
                        "Failed to parse host groups for service {}: {}",
                        service.name, err
                    );
                    continue;
                }
            };
            for host_group in host_groups {
                info!("Service {} checking group {}", service.name, host_group);
                // get the group data
                let group = match host_group::find_by_name(&host_group, db.as_ref()).await {
                    Ok(Some(group)) => group,
                    Ok(None) => {
                        error!("Host group {} not found, this should already have been sorted by the update_db_from_config for host_groups", host_group);
                        continue;
                    }
                    Err(err) => {
                        error!("DB Error finding host group {}: {:?}", host_group, err);
                        continue;
                    }
                };

                let host_group_members = match host_group_members::Entity::find()
                    .filter(host_group_members::Column::GroupId.eq(group.id))
                    .all(db.as_ref())
                    .await
                {
                    Ok(hosts) => hosts,
                    Err(err) => {
                        error!("DB Error finding hosts for group {}: {}", host_group, err);
                        return Err(err.into());
                    }
                };
                for host_group_member in host_group_members {
                    // let's just check we should have that member
                    let host = host::Entity::find_by_id(host_group_member.host_id)
                        .one(db.as_ref())
                        .await?;
                    if host.is_none() {
                        error!(
                            "Host group member {} not found, this should already have been sorted by the update_db_from_config for host",
                            host_group_member.host_id
                        );
                        continue;
                    }

                    // check we have the service check
                    match Entity::find()
                        .filter(Column::HostId.eq(host_group_member.host_id))
                        .filter(Column::ServiceId.eq(service.id))
                        .one(db.as_ref())
                        .await
                        .map_err(Error::from)?
                    {
                        None => {
                            info!(
                                "Adding service check for service {} on host {:?}",
                                service.name, host_group_member
                            );
                            let model = ActiveModel {
                                id: Set(Uuid::new_v4()),
                                service_id: Set(service_id),
                                host_id: Set(host_group_member.host_id),
                                status: Set(ServiceStatus::Unknown),
                                last_check: Set(chrono::Utc::now()),
                                next_check: Set(chrono::Utc::now()),
                                last_updated: Set(chrono::Utc::now()),
                            };
                            debug!("Inserting... {:?}", model);
                            model.insert(db.as_ref()).await.map_err(Error::from)?;
                            debug!("Done!");
                        }
                        Some(service_check) => {
                            debug!("Found existing service check: {:?}", service_check);
                            let mut service_check = service_check.into_active_model();
                            // if the service has been in checking for more than 10 seconds, we'll reset it.
                            if let sea_orm::ActiveValue::Set(last_check) =
                                service_check.last_check.clone()
                            {
                                if last_check + chrono::Duration::seconds(5) < chrono::Utc::now() {
                                    if let sea_orm::ActiveValue::Set(ServiceStatus::Checking) =
                                        service_check.status
                                    {
                                        service_check
                                            .status
                                            .set_if_not_equals(ServiceStatus::Unknown);
                                        // service_check.save(db.as_ref()).await.map_err(Error::from)?;
                                    }
                                }

                                if service_check.is_changed() {
                                    service_check.save(db.as_ref()).await.map_err(Error::from)?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// For when you want to see all the details of a service check
#[derive(Clone, Debug, PartialEq, Eq, FromQueryResult)]

pub struct FullServiceCheck {
    pub service_check_id: Uuid,
    pub service_name: String,
    pub service_type: ServiceType,

    pub host_id: Uuid,
    pub host_name: String,

    pub last_check: DateTime<Utc>,
    pub next_check: DateTime<Utc>,
    pub status: ServiceStatus,
}

impl FullServiceCheck {
    pub async fn all(db: &DatabaseConnection) -> Result<Vec<Self>, Error> {
        Self::all_query()
            .into_model::<FullServiceCheck>()
            .all(db)
            .await
            .map_err(Error::from)
    }

    pub fn all_query() -> Select<Entity> {
        Entity::find()
            .column_as(service::Column::Name, "service_name")
            .column_as(host::Column::Id, "host_id")
            .column_as(host::Column::Hostname, "host_name")
            .column_as(service::Column::Type, "service_type")
            .join(JoinType::LeftJoin, Relation::Service.def())
            .join(JoinType::LeftJoin, Relation::Host.def())
            .column_as(Column::Id, "service_check_id")
    }

    pub fn get_by_service_id_query(service_id: Uuid) -> Select<Entity> {
        Self::all_query().filter(Column::ServiceId.eq(service_id))
    }

    pub async fn get_by_service_id(
        service_id: Uuid,
        db: &DatabaseConnection,
    ) -> Result<Vec<FullServiceCheck>, Error> {
        Self::get_by_service_id_query(service_id)
            .into_model::<FullServiceCheck>()
            .all(db)
            .await
            .map_err(Error::from)
    }
}
