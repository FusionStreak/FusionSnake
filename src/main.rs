use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, middleware, web};
use game_objects::GameState;
use log::info;
use serde_json::json;
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod game_objects;
mod logic;
mod stats;

use stats::{
    ActiveGames, SharedStats, cleanup_stale_games, create_active_games, create_shared_stats,
};

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

async fn handle_stats(data: web::Data<SharedStats>) -> HttpResponse {
    if let Ok(game_stats) = data.lock() {
        HttpResponse::Ok().json(json!({
            "total_games": game_stats.total_games,
            "wins": game_stats.wins,
            "losses": game_stats.losses,
            "draws": game_stats.draws,
            "win_rate": format!("{:.1}", game_stats.win_rate()),
            "total_turns": game_stats.total_turns,
            "average_turns": format!("{:.1}", game_stats.average_turns()),
            "longest_game": game_stats.longest_game,
            "shortest_game": if game_stats.shortest_game == u32::MAX { 0 } else { game_stats.shortest_game },
            "total_food_eaten": game_stats.total_food_eaten,
            "average_food_eaten": format!("{:.1}", game_stats.average_food_eaten()),
            "last_played": game_stats.last_played
        }))
    } else {
        HttpResponse::InternalServerError().json(json!({
            "error": "Failed to acquire stats lock"
        }))
    }
}

async fn handle_start(
    game_state: web::Json<GameState>,
    active_games: web::Data<ActiveGames>,
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
    active_games: web::Data<ActiveGames>,
) -> HttpResponse {
    let response = logic::get_move(
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
    );

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
    stats_data: web::Data<SharedStats>,
    active_games: web::Data<ActiveGames>,
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
            // Fallback if game wasn't tracked (shouldn't happen)
            log::warn!("Game {} not found in active games", game_state.game.id);
            (game_state.turn.cast_unsigned(), 0)
        }
    } else {
        // Fallback if lock fails
        log::error!("Failed to acquire active games lock");
        (game_state.turn.cast_unsigned(), 0)
    };

    // Record the game with accurate stats
    if let Ok(mut game_stats) = stats_data.lock() {
        game_stats.record_game(turns, food_eaten, won, is_draw);
        if let Err(e) = game_stats.save() {
            log::error!("Failed to save stats: {e}");
        }
    }

    HttpResponse::Ok().finish()
}

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

    // Initialize shared stats and active games tracker
    let shared_stats = create_shared_stats();
    let active_games = create_active_games();

    HttpServer::new(move || {
        // Configure CORS to allow requests from any origin
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![actix_web::http::header::CONTENT_TYPE])
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(shared_stats.clone()))
            .app_data(web::Data::new(active_games.clone()))
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .wrap(ServerHeader)
            .route("/", web::get().to(handle_index))
            .route("/stats", web::get().to(handle_stats))
            .route("/start", web::post().to(handle_start))
            .route("/move", web::post().to(handle_move))
            .route("/end", web::post().to(handle_end))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
