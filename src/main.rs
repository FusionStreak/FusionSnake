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

use stats::{SharedStats, create_shared_stats};

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
            "last_played": game_stats.last_played
        }))
    } else {
        HttpResponse::InternalServerError().json(json!({
            "error": "Failed to acquire stats lock"
        }))
    }
}

async fn handle_start(game_state: web::Json<GameState>) -> HttpResponse {
    logic::start(
        &game_state.game,
        &game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    HttpResponse::Ok().finish()
}

async fn handle_move(game_state: web::Json<GameState>) -> HttpResponse {
    let response = logic::get_move(
        &game_state.game,
        &game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    HttpResponse::Ok().json(response)
}

async fn handle_end(
    game_state: web::Json<GameState>,
    data: web::Data<SharedStats>,
) -> HttpResponse {
    let (won, is_draw) = logic::end(
        &game_state.game,
        &game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    // Record the game
    let turns = game_state.turn as u32;
    if let Ok(mut game_stats) = data.lock() {
        game_stats.record_game(turns, won, is_draw);
        if let Err(e) = game_stats.save() {
            log::error!("Failed to save stats: {}", e);
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

    info!("Starting Battlesnake Server on port {}...", port);

    // Initialize shared stats
    let shared_stats = create_shared_stats();

    HttpServer::new(move || {
        // Configure CORS to allow requests from any origin
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![actix_web::http::header::CONTENT_TYPE])
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(shared_stats.clone()))
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
