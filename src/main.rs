use actix_cors::Cors;
use actix_web::web::{Data, get, post};
use actix_web::{App, HttpResponse, HttpServer, middleware, web};
use game_objects::GameState;
use log::info;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod db;
mod game_objects;
mod logic;
mod stats;
mod training;

use stats::{ActiveGames, cleanup_stale_games, create_active_games};
use training::TrainingLogger;

// Middleware to add custom Server header
use actix_web::Error;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use futures_util::future::LocalBoxFuture;
use std::future::{Ready, ready};

pub struct ServerHeader;

impl<S, B> Transform<S, ServiceRequest> for ServerHeader
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ServerHeaderMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ServerHeaderMiddleware { service }))
    }
}

pub struct ServerHeaderMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for ServerHeaderMiddleware<S>
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
            res.headers_mut().insert(
                actix_web::http::header::SERVER,
                actix_web::http::header::HeaderValue::from_static(
                    "battlesnake/github/starter-snake-rust",
                ),
            );
            Ok(res)
        })
    }
}

// API and Response Objects
// See https://docs.battlesnake.com/api

async fn handle_index() -> HttpResponse {
    HttpResponse::Ok().json(logic::info())
}

async fn handle_stats(pool: Data<SqlitePool>) -> HttpResponse {
    HttpResponse::Ok().json(db::get_stats(&pool).await)
}

async fn handle_start(
    game_state: web::Json<GameState>,
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

async fn handle_move(
    game_state: web::Json<GameState>,
    active_games: Data<ActiveGames>,
    training: Data<TrainingLogger>,
) -> HttpResponse {
    let (response, features) = logic::get_move(
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
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

async fn handle_end(
    game_state: web::Json<GameState>,
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

#[derive(Deserialize)]
struct TurnsQuery {
    game_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn handle_training_turns(
    pool: Data<SqlitePool>,
    query: web::Query<TurnsQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    HttpResponse::Ok().json(db::get_turns(&pool, query.game_id.as_deref(), limit, offset).await)
}

#[derive(Deserialize)]
struct PaginationQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn handle_training_outcomes(
    pool: Data<SqlitePool>,
    query: web::Query<PaginationQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    HttpResponse::Ok().json(db::get_outcomes(&pool, limit, offset).await)
}

async fn handle_training_summary(pool: Data<SqlitePool>) -> HttpResponse {
    HttpResponse::Ok().json(db::get_training_summary(&pool).await)
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
}

async fn handle_stats_history(
    pool: Data<SqlitePool>,
    query: web::Query<HistoryQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    HttpResponse::Ok().json(db::get_stats_history(&pool, limit).await)
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

    HttpServer::new(move || {
        // Configure CORS to allow requests from any origin
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![actix_web::http::header::CONTENT_TYPE])
            .max_age(3600);

        App::new()
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(active_games.clone()))
            .app_data(Data::new(training_logger.clone()))
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .wrap(ServerHeader)
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
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
