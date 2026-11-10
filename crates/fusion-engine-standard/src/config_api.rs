use actix_web::HttpResponse;
use actix_web::web::{self, Data, Json};
use log::{error, info};
use utoipa::OpenApi;

use crate::engine::StandardEngine;
use crate::heuristic_params::HeuristicParams;

#[utoipa::path(
    get,
    path = "/config/standard",
    tag = "Config",
    responses(
        (status = 200, description = "Current standard-engine heuristic parameters", body = HeuristicParams),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn handle_query_standard_config(engine: Data<StandardEngine>) -> HttpResponse {
    HttpResponse::Ok().json(engine.get_params().await)
}

#[utoipa::path(
    post,
    path = "/config/standard",
    tag = "Config",
    request_body = HeuristicParams,
    responses(
        (status = 200, description = "Standard-engine parameters updated successfully"),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Disk write failure")
    )
)]
pub async fn handle_set_standard_config(
    engine: Data<StandardEngine>,
    body: Json<HeuristicParams>,
) -> HttpResponse {
    let new_params = body.into_inner();

    if let Err(e) = engine.set_params(new_params).await {
        let status = if e.starts_with("Validation failed:") {
            actix_web::http::StatusCode::BAD_REQUEST
        } else {
            error!("Failed to persist standard-engine params to disk: {e}");
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        };
        return HttpResponse::build(status).json(serde_json::json!({ "error": e }));
    }

    info!("Heuristic parameters updated via POST /config/standard");
    HttpResponse::Ok().json(serde_json::json!({"status": "ok", "message": "Parameters updated"}))
}

#[utoipa::path(
    post,
    path = "/config/standard/reset",
    tag = "Config",
    responses(
        (status = 200, description = "Standard-engine parameters reset to defaults"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Disk write failure")
    )
)]
pub async fn handle_reset_standard_config(engine: Data<StandardEngine>) -> HttpResponse {
    if let Err(e) = engine.reset_params().await {
        error!("Failed to persist standard-engine defaults to disk: {e}");
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Failed to persist defaults to disk: {e}")
        }));
    }

    info!("Heuristic parameters reset to defaults via POST /config/standard/reset");
    HttpResponse::Ok()
        .json(serde_json::json!({"status": "ok", "message": "Parameters reset to defaults"}))
}

pub fn configure_standard_config_routes(cfg: &mut web::ServiceConfig, engine: StandardEngine) {
    cfg.app_data(Data::new(engine))
        .route("/standard", web::get().to(handle_query_standard_config))
        .route("/standard", web::post().to(handle_set_standard_config))
        .route(
            "/standard/reset",
            web::post().to(handle_reset_standard_config),
        );
}

#[derive(OpenApi)]
#[openapi(
    paths(
        handle_query_standard_config,
        handle_set_standard_config,
        handle_reset_standard_config,
    ),
    components(schemas(HeuristicParams))
)]
pub struct StandardConfigApiDoc;
