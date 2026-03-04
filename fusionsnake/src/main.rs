use actix_cors::Cors;
use actix_web::web::{Data, get, post};
use actix_web::{App, HttpResponse, HttpServer, middleware, web};
use game_objects::GameState;
use log::{debug, info};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa::openapi::security::{ApiKey as ApiKeyScheme, ApiKeyValue, SecurityScheme};

mod auth;
mod db;
mod game_objects;
mod heuristic_params;
mod logic;
mod responses;
mod stats;
mod training;

use heuristic_params::{SharedParams, create_shared_params};
use stats::{ActiveGames, cleanup_stale_games, create_active_games};
use training::TrainingLogger;

// ---------------------------------------------------------------------------
// LoggedJson extractor — logs the raw request body for every game endpoint.
// At DEBUG level: always logs the path + byte count + raw payload.
// At WARN level:  logs the full payload when JSON deserialization fails.
// ---------------------------------------------------------------------------

struct LoggedJson<T>(T);

impl<T> std::ops::Deref for LoggedJson<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> actix_web::FromRequest for LoggedJson<T>
where
    T: serde::de::DeserializeOwned + 'static,
{
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        let path = req.path().to_owned();
        let bytes_fut = actix_web::web::Bytes::from_request(req, payload);
        Box::pin(async move {
            let bytes = bytes_fut.await.map_err(|e| {
                log::warn!("Failed to read request body: {e}");
                actix_web::error::ErrorBadRequest(e)
            })?;

            debug!(
                "Incoming payload on {path} ({} bytes): {}",
                bytes.len(),
                String::from_utf8_lossy(&bytes)
            );

            serde_json::from_slice::<T>(&bytes)
                .map(LoggedJson)
                .map_err(|e| {
                    log::warn!(
                        "Failed to deserialize JSON on {path}: {e}\nPayload: {}",
                        String::from_utf8_lossy(&bytes)
                    );
                    actix_web::error::ErrorBadRequest(format!("Invalid JSON: {e}"))
                })
        })
    }
}

// Middleware to add security headers to every response
use actix_web::Error;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use futures_util::future::LocalBoxFuture;
use std::future::{Ready, ready};

pub struct SecurityHeaders;

impl<S, B> Transform<S, ServiceRequest> for SecurityHeaders
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = SecurityHeadersMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SecurityHeadersMiddleware { service }))
    }
}

pub struct SecurityHeadersMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for SecurityHeadersMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;
            let headers = res.headers_mut();

            headers.insert(
                actix_web::http::header::SERVER,
                actix_web::http::header::HeaderValue::from_static("FusionSnake"),
            );
            headers.insert(
                actix_web::http::header::X_CONTENT_TYPE_OPTIONS,
                actix_web::http::header::HeaderValue::from_static("nosniff"),
            );
            headers.insert(
                actix_web::http::header::X_FRAME_OPTIONS,
                actix_web::http::header::HeaderValue::from_static("DENY"),
            );
            headers.insert(
                actix_web::http::header::REFERRER_POLICY,
                actix_web::http::header::HeaderValue::from_static("no-referrer"),
            );
            headers.insert(
                actix_web::http::header::HeaderName::from_static("x-xss-protection"),
                actix_web::http::header::HeaderValue::from_static("0"),
            );
            headers.insert(
                actix_web::http::header::CONTENT_SECURITY_POLICY,
                actix_web::http::header::HeaderValue::from_static("default-src 'none'"),
            );
            headers.insert(
                actix_web::http::header::STRICT_TRANSPORT_SECURITY,
                actix_web::http::header::HeaderValue::from_static(
                    "max-age=31536000; includeSubDomains",
                ),
            );

            Ok(res)
        })
    }
}

// API and Response Objects
// See https://docs.battlesnake.com/api

#[utoipa::path(
    get,
    path = "/",
    tag = "Battlesnake API",
    responses(
        (status = 200, description = "Snake metadata and appearance", body = responses::InfoResponse)
    )
)]
async fn handle_index() -> HttpResponse {
    HttpResponse::Ok().json(logic::info())
}

#[utoipa::path(
    get,
    path = "/stats",
    tag = "Stats",
    responses(
        (status = 200, description = "Aggregate game statistics", body = responses::StatsResponse),
        (status = 500, description = "Database error", body = responses::ErrorResponse)
    )
)]
async fn handle_stats(pool: Data<SqlitePool>) -> HttpResponse {
    match db::get_stats(&pool).await {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(msg) => {
            HttpResponse::InternalServerError().json(responses::ErrorResponse { error: msg })
        }
    }
}

#[utoipa::path(
    post,
    path = "/start",
    tag = "Battlesnake API",
    request_body = GameState,
    responses(
        (status = 200, description = "Game acknowledged")
    )
)]
async fn handle_start(
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
) -> HttpResponse {
    logic::start(
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    // Track this new game
    if let Ok(mut games) = active_games.lock() {
        games.insert(
            game_state.game.id.clone(),
            stats::ActiveGame {
                last_turn: 0,
                started_at: chrono::Utc::now(),
                starting_length: game_state.you.length,
            },
        );

        // Cleanup stale games (older than 6 hours)
        drop(games); // Release the lock before cleanup
        cleanup_stale_games(&active_games, 6 * 60 * 60);
    }

    HttpResponse::Ok().finish()
}

#[utoipa::path(
    post,
    path = "/move",
    tag = "Battlesnake API",
    request_body = GameState,
    responses(
        (status = 200, description = "Chosen move direction", body = responses::MoveResponse)
    )
)]
async fn handle_move(
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
    training: Data<TrainingLogger>,
    shared_params: Data<SharedParams>,
) -> HttpResponse {
    let params = shared_params.read().await;
    let (response, features) = logic::get_move(
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
        &params,
    );

    // Fire-and-forget: insert turn features into SQLite in the background
    training.log_turn(game_state.game.id.clone(), features);

    // Update the last turn for this game
    if let Ok(mut games) = active_games.lock()
        && let Some(game) = games.get_mut(&game_state.game.id)
    {
        game.last_turn = game_state.turn.cast_unsigned();
    }

    HttpResponse::Ok().json(response)
}

#[utoipa::path(
    post,
    path = "/end",
    tag = "Battlesnake API",
    request_body = GameState,
    responses(
        (status = 200, description = "Game ended")
    )
)]
async fn handle_end(
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
    training: Data<TrainingLogger>,
) -> HttpResponse {
    let (won, is_draw) = logic::end(
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    // Get the accurate turn count and calculate food eaten
    let (turns, food_eaten) = if let Ok(mut games) = active_games.lock() {
        if let Some(game) = games.remove(&game_state.game.id) {
            let turns = game.last_turn;
            let food_eaten = game_state.you.length.saturating_sub(game.starting_length);
            (turns, food_eaten)
        } else {
            log::warn!("Game {} not found in active games", game_state.game.id);
            (game_state.turn.cast_unsigned(), 0)
        }
    } else {
        log::error!("Failed to acquire active games lock");
        (game_state.turn.cast_unsigned(), 0)
    };

    // Fire-and-forget: write outcome + aggregate stats to SQLite
    training.log_outcome(game_state.game.id.clone(), won, is_draw, turns, food_eaten);
    training.log_game_stats(turns, food_eaten, won, is_draw);

    HttpResponse::Ok().finish()
}

// ---------------------------------------------------------------------------
// Training data & stats-history query endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize, utoipa::IntoParams)]
struct TurnsQuery {
    /// Filter by game ID.
    game_id: Option<String>,
    /// Maximum number of records (default: 100, max: 1000).
    limit: Option<i64>,
    /// Offset for pagination.
    offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/training/turns",
    tag = "Training",
    params(TurnsQuery),
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Paginated turn feature records", body = responses::PaginatedTurns),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse),
        (status = 500, description = "Database error", body = responses::ErrorResponse)
    )
)]
async fn handle_training_turns(
    _key: auth::ApiKey,
    pool: Data<SqlitePool>,
    query: web::Query<TurnsQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    match db::get_turns(&pool, query.game_id.as_deref(), limit, offset).await {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(msg) => {
            HttpResponse::InternalServerError().json(responses::ErrorResponse { error: msg })
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
struct PaginationQuery {
    /// Maximum number of records (default: 100, max: 1000).
    limit: Option<i64>,
    /// Offset for pagination.
    offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/training/outcomes",
    tag = "Training",
    params(PaginationQuery),
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Paginated game outcome records", body = responses::PaginatedOutcomes),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse),
        (status = 500, description = "Database error", body = responses::ErrorResponse)
    )
)]
async fn handle_training_outcomes(
    _key: auth::ApiKey,
    pool: Data<SqlitePool>,
    query: web::Query<PaginationQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    match db::get_outcomes(&pool, limit, offset).await {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(msg) => {
            HttpResponse::InternalServerError().json(responses::ErrorResponse { error: msg })
        }
    }
}

#[utoipa::path(
    get,
    path = "/training/summary",
    tag = "Training",
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Aggregate training data summary", body = responses::TrainingSummary),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse)
    )
)]
async fn handle_training_summary(_key: auth::ApiKey, pool: Data<SqlitePool>) -> HttpResponse {
    HttpResponse::Ok().json(db::get_training_summary(&pool).await)
}

#[derive(Deserialize, utoipa::IntoParams)]
struct HistoryQuery {
    /// Maximum number of records (default: 100, max: 1000).
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/stats/history",
    tag = "Stats",
    params(HistoryQuery),
    responses(
        (status = 200, description = "Per-game stats with cumulative aggregates", body = responses::PaginatedStatsHistory),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse),
        (status = 500, description = "Database error", body = responses::ErrorResponse)
    )
)]
async fn handle_stats_history(
    pool: Data<SqlitePool>,
    query: web::Query<HistoryQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    match db::get_stats_history(&pool, limit).await {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(msg) => {
            HttpResponse::InternalServerError().json(responses::ErrorResponse { error: msg })
        }
    }
}

// ---------------------------------------------------------------------------
// Heuristic parameter configuration endpoints
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/config",
    tag = "Config",
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Current heuristic parameters", body = heuristic_params::HeuristicParams),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse)
    )
)]
async fn handle_get_config(_key: auth::ApiKey, shared_params: Data<SharedParams>) -> HttpResponse {
    let params = shared_params.read().await;
    HttpResponse::Ok().json(params.clone())
}

#[utoipa::path(
    post,
    path = "/config",
    tag = "Config",
    security(("api_key" = [])),
    request_body = heuristic_params::HeuristicParams,
    responses(
        (status = 200, description = "Parameters updated successfully"),
        (status = 400, description = "Validation error", body = responses::ErrorResponse),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse)
    )
)]
async fn handle_set_config(
    _key: auth::ApiKey,
    shared_params: Data<SharedParams>,
    body: web::Json<heuristic_params::HeuristicParams>,
) -> HttpResponse {
    let new_params = body.into_inner();

    // Validate before applying
    let errors = new_params.validate();
    if !errors.is_empty() {
        return HttpResponse::BadRequest().json(responses::ErrorResponse {
            error: format!("Validation failed: {}", errors.join("; ")),
        });
    }

    // Persist to disk first so we don't lose params on crash
    let path =
        std::env::var("PARAMS_FILE").unwrap_or_else(|_| heuristic_params::PARAMS_FILE.to_string());
    if let Err(e) = new_params.save_to_file(std::path::Path::new(&path)) {
        log::warn!("Failed to persist params to disk: {e}");
        // Continue anyway — in-memory update is still valid
    }

    // Store previous params for potential rollback
    let mut params = shared_params.write().await;
    *params = new_params;
    drop(params);

    info!("Heuristic parameters updated via POST /config");
    HttpResponse::Ok().json(serde_json::json!({"status": "ok", "message": "Parameters updated"}))
}

#[utoipa::path(
    post,
    path = "/config/reset",
    tag = "Config",
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Parameters reset to defaults"),
        (status = 401, description = "Unauthorized", body = responses::ErrorResponse)
    )
)]
async fn handle_reset_config(
    _key: auth::ApiKey,
    shared_params: Data<SharedParams>,
) -> HttpResponse {
    let defaults = heuristic_params::HeuristicParams::default();

    let path =
        std::env::var("PARAMS_FILE").unwrap_or_else(|_| heuristic_params::PARAMS_FILE.to_string());
    let _ = defaults.save_to_file(std::path::Path::new(&path));

    let mut params = shared_params.write().await;
    *params = defaults;

    info!("Heuristic parameters reset to defaults via POST /config/reset");
    HttpResponse::Ok()
        .json(serde_json::json!({"status": "ok", "message": "Parameters reset to defaults"}))
}

// ---------------------------------------------------------------------------
// OpenAPI specification
// ---------------------------------------------------------------------------

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                SecurityScheme::ApiKey(ApiKeyScheme::Header(ApiKeyValue::new("X-API-Key"))),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "FusionSnake API",
        description = "Battlesnake bot with ML training data collection and statistics",
        version = "0.1.0",
        license(name = "MIT")
    ),
    paths(
        handle_index,
        handle_start,
        handle_move,
        handle_end,
        handle_stats,
        handle_stats_history,
        handle_training_turns,
        handle_training_outcomes,
        handle_training_summary,
        handle_get_config,
        handle_set_config,
        handle_reset_config,
    ),
    components(schemas(
        responses::InfoResponse,
        responses::MoveResponse,
        responses::ErrorResponse,
        responses::StatsResponse,
        responses::TurnRecord,
        responses::OutcomeRecord,
        responses::StatsHistoryRecord,
        responses::PaginatedTurns,
        responses::PaginatedOutcomes,
        responses::PaginatedStatsHistory,
        responses::TrainingAverages,
        responses::TrainingSummary,
        heuristic_params::HeuristicParams,
        game_objects::GameState,
        game_objects::Game,
        game_objects::Ruleset,
        game_objects::RulesetSettings,
        game_objects::RoyaleSettings,
        game_objects::SquadSettings,
        game_objects::Board,
        game_objects::Battlesnake,
        game_objects::Customization,
        game_objects::Coord,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "Battlesnake API", description = "Core Battlesnake game engine endpoints"),
        (name = "Stats", description = "Game statistics and history"),
        (name = "Training", description = "ML training data endpoints (API key required)"),
        (name = "Config", description = "Runtime heuristic parameter tuning (API key required)")
    )
)]
struct ApiDoc;

async fn handle_openapi() -> HttpResponse {
    HttpResponse::Ok().json(ApiDoc::openapi())
}

// ---------------------------------------------------------------------------
// ML trainer periodic trigger
// ---------------------------------------------------------------------------

/// Spawns a detached background task that:
/// 1. Polls `GET {trainer_url}/health` with exponential backoff until the
///    trainer is reachable — the trainer container starts Flask immediately
///    with no startup training of its own.
/// 2. Fires an immediate `POST {trainer_url}/train` once the trainer is healthy
///    to kick off the first training pass.
/// 3. Enters a fixed 24-hour `interval_at` loop for subsequent triggers.
///
/// All three phases run inside a single detached Tokio task — the HTTP server
/// is never blocked. Trainer downtime between periodic triggers is tolerated:
/// a failed POST is logged as a warning and the loop continues.
fn spawn_trainer_trigger(trainer_url: String) {
    drop(tokio::spawn(async move {
        let client = reqwest::Client::new();
        let health_url = format!("{trainer_url}/health");
        let train_url = format!("{trainer_url}/train");

        // ── Phase 1: await trainer readiness ─────────────────────────────
        info!("Waiting for ML trainer to become reachable at {health_url}");
        let mut backoff = std::time::Duration::from_secs(5);
        let backoff_cap = std::time::Duration::from_secs(60);
        loop {
            match client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("ML trainer is healthy — proceeding with initial trigger");
                    break;
                }
                Ok(resp) => {
                    info!(
                        "ML trainer not yet healthy ({}), retrying in {}s",
                        resp.status(),
                        backoff.as_secs()
                    );
                }
                Err(e) => {
                    info!(
                        "ML trainer not yet reachable ({e}), retrying in {}s",
                        backoff.as_secs()
                    );
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(backoff_cap);
        }

        // ── Phase 2: initial training trigger ────────────────────────────
        info!("Sending initial training trigger to {train_url}");
        match client.post(&train_url).send().await {
            Ok(resp) => info!("Trainer responded with {} (initial)", resp.status()),
            Err(e) => log::warn!("Initial training trigger failed: {e}"),
        }

        // ── Phase 3: periodic 24-hour trigger ────────────────────────────
        let period = std::time::Duration::from_secs(24 * 60 * 60);
        let start = tokio::time::Instant::now() + period;
        let mut interval = tokio::time::interval_at(start, period);
        loop {
            interval.tick().await;
            info!("Triggering ML training pipeline at {train_url}");
            match client.post(&train_url).send().await {
                Ok(resp) => info!("Trainer responded with {}", resp.status()),
                Err(e) => log::warn!("Failed to trigger trainer: {e}"),
            }
        }
    }));
}

// ---------------------------------------------------------------------------
// Server entry-point
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let port = env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(6666);

    // Initialize JSON logging
    let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().json())
        .with(tracing_subscriber::EnvFilter::new(log_level))
        .init();

    info!("Starting Battlesnake Server on port {port}...");

    // Initialize SQLite pool, active-game tracker, and training logger
    let pool = db::init().await;
    let active_games = create_active_games();
    let training_logger = TrainingLogger::new(pool.clone());
    let shared_params = create_shared_params();

    // Kick off the 24-hour trainer trigger if a URL is configured
    if let Ok(trainer_url) = env::var("TRAINER_URL") {
        info!("Trainer trigger configured: {trainer_url}/train will be called every 24 h");
        spawn_trainer_trigger(trainer_url);
    }

    HttpServer::new(move || {
        // Configure CORS to allow requests from any origin
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::HeaderName::from_static("x-api-key"),
            ])
            .max_age(3600);

        App::new()
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(active_games.clone()))
            .app_data(Data::new(training_logger.clone()))
            .app_data(Data::new(shared_params.clone()))
            .app_data(
                web::JsonConfig::default()
                    .limit(262_144) // 256 KB
                    .error_handler(|err, req| {
                        let message = match &err {
                            actix_web::error::JsonPayloadError::ContentType => {
                                debug!("Invalid Content-Type: expected application/json");
                                "Content-Type must be application/json".to_string()
                            }
                            actix_web::error::JsonPayloadError::Deserialize(e) => {
                                debug!("Failed to deserialize JSON on {}: {e}", req.path());
                                "Invalid request body".to_string()
                            }
                            actix_web::error::JsonPayloadError::Payload(_) => {
                                debug!("Failed to read request body {err}");
                                "Failed to read request body".to_string()
                            }
                            _ => "Bad request".to_string(),
                        };
                        actix_web::error::InternalError::from_response(
                            err,
                            HttpResponse::BadRequest()
                                .json(responses::ErrorResponse { error: message }),
                        )
                        .into()
                    }),
            )
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .wrap(SecurityHeaders)
            // Battlesnake API
            .route("/", get().to(handle_index))
            .route("/start", post().to(handle_start))
            .route("/move", post().to(handle_move))
            .route("/end", post().to(handle_end))
            // Stats & training data
            .route("/stats", get().to(handle_stats))
            .route("/stats/history", get().to(handle_stats_history))
            .route("/training/turns", get().to(handle_training_turns))
            .route("/training/outcomes", get().to(handle_training_outcomes))
            .route("/training/summary", get().to(handle_training_summary))
            // Config (heuristic params)
            .route("/config", get().to(handle_get_config))
            .route("/config", post().to(handle_set_config))
            .route("/config/reset", post().to(handle_reset_config))
            // OpenAPI spec
            .route("/api-doc/openapi.json", get().to(handle_openapi))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
