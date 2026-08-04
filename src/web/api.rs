//! Versioned JSON API for bearer-token automation.

use std::collections::HashMap;

use axum::{
    extract::{Path, Request, State},
    http::{header::WWW_AUTHENTICATE, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use utoipa::{Modify, OpenApi, ToSchema};

use crate::constants::DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES;
use crate::db::entities::{
    api_token, host, host_group, service, service_check, service_check_history,
};
use crate::errors::MaremmaError;
use crate::host::HostCheck;
use crate::prelude::*;
use crate::services::{ServiceStatus, ServiceType};
use crate::web::WebState;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum ApiErrorCode {
    Unauthorized,
    NotFound,
    Validation,
    Internal,
}

#[derive(Serialize, ToSchema)]
struct ApiErrorResponse {
    code: ApiErrorCode,
    message: String,
}

pub(crate) enum ApiError {
    Unauthorized,
    NotFound,
    Validation(String),
    Internal,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn response_body(&self) -> ApiErrorResponse {
        match self {
            Self::Unauthorized => ApiErrorResponse {
                code: ApiErrorCode::Unauthorized,
                message: "A valid bearer token is required".to_string(),
            },
            Self::NotFound => ApiErrorResponse {
                code: ApiErrorCode::NotFound,
                message: "The requested resource was not found".to_string(),
            },
            Self::Validation(message) => ApiErrorResponse {
                code: ApiErrorCode::Validation,
                message: message.clone(),
            },
            Self::Internal => ApiErrorResponse {
                code: ApiErrorCode::Internal,
                message: "An internal server error occurred".to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut response = (status, Json(self.response_body())).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl From<MaremmaError> for ApiError {
    fn from(error: MaremmaError) -> Self {
        match error {
            MaremmaError::Unauthorized => Self::Unauthorized,
            MaremmaError::HostGroupMembershipNotFound(_, _)
            | MaremmaError::HostGroupNotFound(_)
            | MaremmaError::HostGroupNotFoundByName(_)
            | MaremmaError::HostNotFound(_)
            | MaremmaError::ServiceCheckNotFound(_)
            | MaremmaError::ServiceNotFound(_)
            | MaremmaError::ServiceNotFoundByName(_) => Self::NotFound,
            MaremmaError::InvalidInput(message) => Self::Validation(message),
            error => {
                error!(?error, "API request failed");
                Self::Internal
            }
        }
    }
}

#[derive(Serialize, ToSchema)]
struct HostResponse {
    id: Uuid,
    name: String,
    hostname: String,
    check: HostCheck,
}

impl From<host::Model> for HostResponse {
    fn from(host: host::Model) -> Self {
        Self {
            id: host.id,
            name: host.name,
            hostname: host.hostname,
            check: host.check,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct HostGroupResponse {
    id: Uuid,
    name: String,
}

impl From<host_group::Model> for HostGroupResponse {
    fn from(host_group: host_group::Model) -> Self {
        Self {
            id: host_group.id,
            name: host_group.name,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct ServiceResponse {
    id: Uuid,
    name: String,
    description: Option<String>,
    service_type: ServiceType,
    cron_schedule: String,
}

impl From<service::Model> for ServiceResponse {
    fn from(service: service::Model) -> Self {
        Self {
            id: service.id,
            name: service.name,
            description: service.description,
            service_type: service.service_type,
            cron_schedule: service.cron_schedule,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct ServiceCheckHistoryResponse {
    id: Uuid,
    timestamp: DateTime<Utc>,
    status: ServiceStatus,
    time_elapsed_ms: i64,
    result_text: String,
}

impl From<service_check_history::Model> for ServiceCheckHistoryResponse {
    fn from(history: service_check_history::Model) -> Self {
        Self {
            id: history.id,
            timestamp: history.timestamp,
            status: history.status,
            time_elapsed_ms: history.time_elapsed,
            result_text: history.result_text,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct ServiceCheckResponse {
    id: Uuid,
    service_id: Uuid,
    service_name: String,
    service_type: ServiceType,
    host_id: Uuid,
    host_name: String,
    status: ServiceStatus,
    last_check: DateTime<Utc>,
    next_check: DateTime<Utc>,
    last_updated: DateTime<Utc>,
    history: Option<Vec<ServiceCheckHistoryResponse>>,
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id)
        .map_err(|_| ApiError::Validation("The resource ID must be a UUID".to_string()))
}

fn service_check_response(
    check: &service_check::Model,
    host: &host::Model,
    service: &service::Model,
    history: Option<Vec<ServiceCheckHistoryResponse>>,
) -> ServiceCheckResponse {
    ServiceCheckResponse {
        id: check.id,
        service_id: service.id,
        service_name: service.name.clone(),
        service_type: service.service_type.clone(),
        host_id: host.id,
        host_name: host.hostname.clone(),
        status: check.status,
        last_check: check.last_check,
        next_check: check.next_check,
        last_updated: check.last_updated,
        history,
    }
}

async fn load_service_check_response(
    state: &WebState,
    check: service_check::Model,
    include_history: bool,
) -> Result<ServiceCheckResponse, ApiError> {
    let host = host::Entity::find_by_id(check.host_id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .ok_or(ApiError::NotFound)?;
    let service = service::Entity::find_by_id(check.service_id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .ok_or(ApiError::NotFound)?;
    let history = if include_history {
        Some(
            service_check_history::Entity::find()
                .filter(service_check_history::Column::ServiceCheckId.eq(check.id))
                .order_by_desc(service_check_history::Column::Timestamp)
                .limit(DEFAULT_SERVICE_CHECK_HISTORY_VIEW_ENTRIES)
                .all(state.db())
                .await
                .map_err(MaremmaError::from)?
                .into_iter()
                .map(ServiceCheckHistoryResponse::from)
                .collect(),
        )
    } else {
        None
    };

    Ok(service_check_response(&check, &host, &service, history))
}

async fn find_service_check(state: &WebState, id: Uuid) -> Result<service_check::Model, ApiError> {
    service_check::Entity::find_by_id(id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    get,
    path = "/api/v1/hosts",
    tag = "Hosts",
    responses((status = 200, body = Vec<HostResponse>), (status = 401, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn list_hosts(State(state): State<WebState>) -> Result<Json<Vec<HostResponse>>, ApiError> {
    host::Entity::find()
        .order_by_asc(host::Column::Name)
        .all(state.db())
        .await
        .map(|hosts| hosts.into_iter().map(HostResponse::from).collect())
        .map(Json)
        .map_err(MaremmaError::from)
        .map_err(ApiError::from)
}

#[utoipa::path(
    get,
    path = "/api/v1/hosts/{id}",
    tag = "Hosts",
    params(("id" = Uuid, Path, description = "Host UUID")),
    responses((status = 200, body = HostResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn get_host(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<HostResponse>, ApiError> {
    let id = parse_id(&id)?;
    host::Entity::find_by_id(id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .map(HostResponse::from)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    get,
    path = "/api/v1/host-groups",
    tag = "Host groups",
    responses((status = 200, body = Vec<HostGroupResponse>), (status = 401, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn list_host_groups(
    State(state): State<WebState>,
) -> Result<Json<Vec<HostGroupResponse>>, ApiError> {
    host_group::Entity::find()
        .order_by_asc(host_group::Column::Name)
        .all(state.db())
        .await
        .map(|groups| groups.into_iter().map(HostGroupResponse::from).collect())
        .map(Json)
        .map_err(MaremmaError::from)
        .map_err(ApiError::from)
}

#[utoipa::path(
    get,
    path = "/api/v1/host-groups/{id}",
    tag = "Host groups",
    params(("id" = Uuid, Path, description = "Host-group UUID")),
    responses((status = 200, body = HostGroupResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn get_host_group(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<HostGroupResponse>, ApiError> {
    let id = parse_id(&id)?;
    host_group::Entity::find_by_id(id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .map(HostGroupResponse::from)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    get,
    path = "/api/v1/services",
    tag = "Services",
    responses((status = 200, body = Vec<ServiceResponse>), (status = 401, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn list_services(
    State(state): State<WebState>,
) -> Result<Json<Vec<ServiceResponse>>, ApiError> {
    service::Entity::find()
        .order_by_asc(service::Column::Name)
        .all(state.db())
        .await
        .map(|services| services.into_iter().map(ServiceResponse::from).collect())
        .map(Json)
        .map_err(MaremmaError::from)
        .map_err(ApiError::from)
}

#[utoipa::path(
    get,
    path = "/api/v1/services/{id}",
    tag = "Services",
    params(("id" = Uuid, Path, description = "Service UUID")),
    responses((status = 200, body = ServiceResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn get_service(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<ServiceResponse>, ApiError> {
    let id = parse_id(&id)?;
    service::Entity::find_by_id(id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .map(ServiceResponse::from)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    get,
    path = "/api/v1/service-checks",
    tag = "Service checks",
    responses((status = 200, body = Vec<ServiceCheckResponse>), (status = 401, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn list_service_checks(
    State(state): State<WebState>,
) -> Result<Json<Vec<ServiceCheckResponse>>, ApiError> {
    let checks = service_check::Entity::find()
        .order_by_desc(service_check::Column::LastUpdated)
        .all(state.db())
        .await
        .map_err(MaremmaError::from)?;
    let hosts = host::Entity::find()
        .all(state.db())
        .await
        .map_err(MaremmaError::from)?
        .into_iter()
        .map(|host| (host.id, host))
        .collect::<HashMap<_, _>>();
    let services = service::Entity::find()
        .all(state.db())
        .await
        .map_err(MaremmaError::from)?
        .into_iter()
        .map(|service| (service.id, service))
        .collect::<HashMap<_, _>>();
    let mut responses = Vec::with_capacity(checks.len());
    for check in checks {
        let host = hosts.get(&check.host_id).ok_or(ApiError::NotFound)?;
        let service = services.get(&check.service_id).ok_or(ApiError::NotFound)?;
        responses.push(service_check_response(&check, host, service, None));
    }

    Ok(Json(responses))
}

#[utoipa::path(
    get,
    path = "/api/v1/service-checks/{id}",
    tag = "Service checks",
    params(("id" = Uuid, Path, description = "Service-check UUID")),
    responses((status = 200, body = ServiceCheckResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn get_service_check(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<ServiceCheckResponse>, ApiError> {
    let check = find_service_check(&state, parse_id(&id)?).await?;
    load_service_check_response(&state, check, true)
        .await
        .map(Json)
}

async fn update_service_check_status(
    state: WebState,
    id: String,
    status: ServiceStatus,
) -> Result<Json<ServiceCheckResponse>, ApiError> {
    let check = find_service_check(&state, parse_id(&id)?).await?;
    let check = check.set_status(status, state.db()).await?;
    load_service_check_response(&state, check, false)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/service-checks/{id}/urgent",
    tag = "Service checks",
    params(("id" = Uuid, Path, description = "Service-check UUID")),
    responses((status = 200, body = ServiceCheckResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn mark_service_check_urgent(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<ServiceCheckResponse>, ApiError> {
    update_service_check_status(state, id, ServiceStatus::Urgent).await
}

#[utoipa::path(
    post,
    path = "/api/v1/service-checks/{id}/enable",
    tag = "Service checks",
    params(("id" = Uuid, Path, description = "Service-check UUID")),
    responses((status = 200, body = ServiceCheckResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn enable_service_check(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<ServiceCheckResponse>, ApiError> {
    update_service_check_status(state, id, ServiceStatus::Pending).await
}

#[utoipa::path(
    post,
    path = "/api/v1/service-checks/{id}/disable",
    tag = "Service checks",
    params(("id" = Uuid, Path, description = "Service-check UUID")),
    responses((status = 200, body = ServiceCheckResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn disable_service_check(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<ServiceCheckResponse>, ApiError> {
    update_service_check_status(state, id, ServiceStatus::Disabled).await
}

#[utoipa::path(
    delete,
    path = "/api/v1/service-checks/{id}",
    tag = "Service checks",
    params(("id" = Uuid, Path, description = "Service-check UUID")),
    responses((status = 204, description = "Service check deleted"), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 404, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse))
)]
async fn delete_service_check(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let result = service_check::Entity::delete_by_id(id)
        .exec(state.db())
        .await
        .map_err(MaremmaError::from)?;
    if result.rows_affected == 0 {
        return Err(ApiError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn require_bearer(
    State(state): State<WebState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let encoded_token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_bearer_header)
        .ok_or(ApiError::Unauthorized)?;
    let authenticated = state
        .auth()
        .map_err(ApiError::from)?
        .validate(encoded_token, Utc::now())
        .map_err(|_| ApiError::Unauthorized)?;
    let token = api_token::Entity::find_by_id(authenticated.token_id)
        .one(state.db())
        .await
        .map_err(MaremmaError::from)?
        .ok_or(ApiError::Unauthorized)?;
    if token.subject != authenticated.subject || !token.is_active_at(Utc::now()) {
        return Err(ApiError::Unauthorized);
    }

    request.extensions_mut().insert(authenticated);
    Ok(next.run(request).await)
}

fn parse_bearer_header(value: &str) -> Option<&str> {
    let mut parts = value.split_ascii_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    (scheme.eq_ignore_ascii_case("bearer") && parts.next().is_none()).then_some(token)
}

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/api/v1/hosts", get(list_hosts))
        .route("/api/v1/hosts/{id}", get(get_host))
        .route("/api/v1/host-groups", get(list_host_groups))
        .route("/api/v1/host-groups/{id}", get(get_host_group))
        .route("/api/v1/services", get(list_services))
        .route("/api/v1/services/{id}", get(get_service))
        .route("/api/v1/service-checks", get(list_service_checks))
        .route(
            "/api/v1/service-checks/{id}",
            get(get_service_check).delete(delete_service_check),
        )
        .route(
            "/api/v1/service-checks/{id}/urgent",
            post(mark_service_check_urgent),
        )
        .route(
            "/api/v1/service-checks/{id}/enable",
            post(enable_service_check),
        )
        .route(
            "/api/v1/service-checks/{id}/disable",
            post(disable_service_check),
        )
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_hosts,
        get_host,
        list_host_groups,
        get_host_group,
        list_services,
        get_service,
        list_service_checks,
        get_service_check,
        mark_service_check_urgent,
        enable_service_check,
        disable_service_check,
        delete_service_check,
    ),
    components(schemas(
        ApiErrorCode,
        ApiErrorResponse,
        HostResponse,
        HostGroupResponse,
        ServiceResponse,
        ServiceCheckHistoryResponse,
        ServiceCheckResponse,
        HostCheck,
        ServiceStatus,
        ServiceType,
    )),
    tags(
        (name = "Hosts", description = "Host inventory"),
        (name = "Host groups", description = "Host-group inventory"),
        (name = "Services", description = "Service inventory"),
        (name = "Service checks", description = "Service-check status and controls"),
    ),
    security(("bearerAuth" = [])),
    modifiers(&SecurityAddon)
)]
pub(crate) struct ApiDoc;

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    use tower::ServiceExt;
    use utoipa::OpenApi;

    use crate::db::entities::{api_token, host, host_group, service, service_check};
    use crate::db::tests::test_setup;
    use crate::web::auth;
    use crate::web::{build_test_app, WebState};

    use super::*;

    async fn issue_test_token(
        state: &WebState,
        subject: &str,
        expires_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> String {
        let issued_at = Utc::now() - Duration::minutes(1);
        let token_id = Uuid::new_v4();
        api_token::ActiveModel {
            id: Set(token_id),
            subject: Set(subject.to_string()),
            name: Set("test token".to_string()),
            issued_at: Set(issued_at),
            expires_at: Set(expires_at),
            revoked_at: Set(revoked_at),
        }
        .insert(state.db())
        .await
        .expect("Failed to insert API token");

        auth::test_auth()
            .expect("Failed to initialize JWT auth")
            .issue(subject.to_string(), token_id, issued_at, expires_at)
            .expect("Failed to issue JWT")
    }

    #[test]
    fn openapi_includes_bearer_auth_and_service_check_paths() {
        let document =
            serde_json::to_value(ApiDoc::openapi()).expect("Failed to serialize OpenAPI document");
        assert!(document
            .pointer("/components/securitySchemes/bearerAuth")
            .is_some());
        for path in [
            "/paths/~1api~1v1~1hosts",
            "/paths/~1api~1v1~1hosts~1{id}",
            "/paths/~1api~1v1~1host-groups",
            "/paths/~1api~1v1~1host-groups~1{id}",
            "/paths/~1api~1v1~1services",
            "/paths/~1api~1v1~1services~1{id}",
            "/paths/~1api~1v1~1service-checks",
            "/paths/~1api~1v1~1service-checks~1{id}",
            "/paths/~1api~1v1~1service-checks~1{id}~1urgent",
            "/paths/~1api~1v1~1service-checks~1{id}~1enable",
            "/paths/~1api~1v1~1service-checks~1{id}~1disable",
        ] {
            assert!(
                document.pointer(path).is_some(),
                "Missing OpenAPI path {path}"
            );
        }
        for schema in [
            "/components/schemas/ApiErrorResponse",
            "/components/schemas/HostResponse",
            "/components/schemas/HostGroupResponse",
            "/components/schemas/ServiceResponse",
            "/components/schemas/ServiceCheckResponse",
        ] {
            assert!(
                document.pointer(schema).is_some(),
                "Missing OpenAPI schema {schema}"
            );
        }

        let paths = document
            .get("paths")
            .and_then(serde_json::Value::as_object)
            .expect("OpenAPI paths should be an object");
        for (path, path_item) in paths {
            for method in ["get", "post", "delete"] {
                let Some(operation) = path_item.get(method) else {
                    continue;
                };
                assert!(
                    operation.pointer("/responses/500").is_some(),
                    "Missing documented 500 response for {method} {path}"
                );

                if path.contains("{id}") {
                    let id_parameter = operation
                        .get("parameters")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|parameters| {
                            parameters.iter().find(|parameter| {
                                parameter.get("name").and_then(serde_json::Value::as_str)
                                    == Some("id")
                            })
                        })
                        .expect("ID operation should document its path parameter");
                    assert_eq!(
                        id_parameter
                            .pointer("/schema/format")
                            .and_then(serde_json::Value::as_str),
                        Some("uuid"),
                        "ID parameter should use UUID format for {method} {path}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn bearer_api_rejects_missing_expired_and_revoked_tokens() {
        let (db, config) = test_setup().await.expect("Failed to set up test state");
        let state = WebState::new(db, config, None, None, std::path::PathBuf::new());
        let app = build_test_app(state.clone())
            .await
            .expect("Failed to build test app");

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/hosts")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .expect("Missing bearer challenge"),
            "Bearer"
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read error response");
        assert!(String::from_utf8_lossy(&body).contains("unauthorized"));

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/hosts")
                    .header(header::AUTHORIZATION, "Bearer not-a-jwt")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let expired = issue_test_token(
            &state,
            "test-subject",
            Utc::now() - Duration::minutes(1),
            None,
        )
        .await;
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/hosts")
                    .header(header::AUTHORIZATION, format!("Bearer {expired}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let revoked = issue_test_token(
            &state,
            "test-subject",
            Utc::now() + Duration::hours(1),
            Some(Utc::now()),
        )
        .await;
        let valid = issue_test_token(
            &state,
            "test-subject",
            Utc::now() + Duration::hours(1),
            None,
        )
        .await;
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/hosts")
                    .header(header::AUTHORIZATION, format!("bearer  {valid}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::get("/api/v1/hosts")
                    .header(header::AUTHORIZATION, format!("Bearer {revoked}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bearer_api_reads_inventory_and_operates_on_service_checks() {
        let (db, config) = test_setup().await.expect("Failed to set up test state");
        let state = WebState::new(db.clone(), config, None, None, std::path::PathBuf::new());
        let app = build_test_app(state.clone())
            .await
            .expect("Failed to build test app");
        let token = issue_test_token(
            &state,
            "test-subject",
            Utc::now() + Duration::hours(1),
            None,
        )
        .await;

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/hosts")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read inventory response");
        assert!(!String::from_utf8_lossy(&body).contains("\"config\""));

        let host = host::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query host")
            .expect("Expected host");
        let host_group = host_group::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query host group")
            .expect("Expected host group");
        let service = service::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query service")
            .expect("Expected service");
        let check = service_check::Entity::find()
            .one(state.db())
            .await
            .expect("Failed to query service check")
            .expect("Expected service check");

        for path in [
            format!("/api/v1/hosts/{}", host.id),
            "/api/v1/host-groups".to_string(),
            format!("/api/v1/host-groups/{}", host_group.id),
            "/api/v1/services".to_string(),
            format!("/api/v1/services/{}", service.id),
            "/api/v1/service-checks".to_string(),
            format!("/api/v1/service-checks/{}", check.id),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(&path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .expect("Failed to build request"),
                )
                .await
                .expect("Failed to call API");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Unexpected status for {path}"
            );
        }

        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/service-checks/{}/urgent", check.id))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::OK);
        let updated = service_check::Entity::find_by_id(check.id)
            .one(state.db())
            .await
            .expect("Failed to load updated check")
            .expect("Service check disappeared");
        assert_eq!(updated.status, ServiceStatus::Urgent);

        for action in ["enable", "disable"] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/api/v1/service-checks/{}/{action}", check.id))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .expect("Failed to build request"),
                )
                .await
                .expect("Failed to call API");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Unexpected status for {action}"
            );
        }

        let response = app
            .clone()
            .oneshot(
                Request::delete(format!("/api/v1/service-checks/{}", check.id))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app
            .oneshot(
                Request::get(format!("/api/v1/service-checks/{}", check.id))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to call API");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn api_documentation_routes_are_public() {
        let (db, config) = test_setup().await.expect("Failed to set up test state");
        let state = WebState::new(db, config, None, None, std::path::PathBuf::new());
        let app = build_test_app(state)
            .await
            .expect("Failed to build test app");

        let openapi = app
            .clone()
            .oneshot(
                Request::get("/api-docs/openapi.json")
                    .body(Body::empty())
                    .expect("Failed to build OpenAPI request"),
            )
            .await
            .expect("Failed to call OpenAPI route");
        assert_eq!(openapi.status(), StatusCode::OK);

        let swagger = app
            .oneshot(
                Request::get("/swagger-ui/")
                    .body(Body::empty())
                    .expect("Failed to build Swagger request"),
            )
            .await
            .expect("Failed to call Swagger route");
        assert_eq!(swagger.status(), StatusCode::OK);
    }
}
