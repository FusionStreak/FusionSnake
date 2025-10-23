#[macro_use]
extern crate rocket;

use game_objects::GameState;
use log::info;
use rocket::fairing::AdHoc;
use rocket::http::{Header, Status};
use rocket::serde::json::Json;
use rocket::{Request, Response, State};
use serde_json::{Value, json};
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod game_objects;
mod logic;
mod stats;

use stats::{SharedStats, create_shared_stats};

// API and Response Objects
// See https://docs.battlesnake.com/api

#[get("/")]
fn handle_index() -> Json<Value> {
    Json(logic::info())
}

// CORS Fairing to allow cross-origin requests from portfolio website
pub struct Cors;

#[rocket::async_trait]
impl rocket::fairing::Fairing for Cors {
    fn info(&self) -> rocket::fairing::Info {
        rocket::fairing::Info {
            name: "Add CORS headers to responses",
            kind: rocket::fairing::Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "GET, POST, OPTIONS",
        ));
        response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type"));
    }
}

#[get("/stats")]
fn handle_stats(stats: &State<SharedStats>) -> Json<Value> {
    if let Ok(game_stats) = stats.lock() {
        Json(json!({
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
        Json(json!({
            "error": "Failed to acquire stats lock"
        }))
    }
}

#[post("/start", format = "json", data = "<start_req>")]
fn handle_start(start_req: Json<GameState>, _stats: &State<SharedStats>) -> Status {
    logic::start(
        &start_req.game,
        &start_req.turn,
        &start_req.board,
        &start_req.you,
    );

    // Initialize game tracking if needed in the future
    // For now, we just acknowledge the start

    Status::Ok
}

#[post("/move", format = "json", data = "<move_req>")]
fn handle_move(move_req: Json<GameState>) -> Json<Value> {
    let response = logic::get_move(
        &move_req.game,
        &move_req.turn,
        &move_req.board,
        &move_req.you,
    );

    Json(response)
}

#[post("/end", format = "json", data = "<end_req>")]
fn handle_end(end_req: Json<GameState>, stats: &State<SharedStats>) -> Status {
    let (won, is_draw) = logic::end(&end_req.game, &end_req.turn, &end_req.board, &end_req.you);

    // Record the game
    let turns = end_req.turn as u32;
    if let Ok(mut game_stats) = stats.lock() {
        game_stats.record_game(turns, won, is_draw);
        if let Err(e) = game_stats.save() {
            log::error!("Failed to save stats: {}", e);
        }
    }

    Status::Ok
}

#[launch]
fn rocket() -> _ {
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

    info!("Starting Battlesnake Server...");

    // Initialize shared stats
    let shared_stats = create_shared_stats();

    // Build the Rocket instance with the specified port.
    rocket::custom(rocket::Config::figment().merge(("port", port)))
        .attach(AdHoc::on_response("Server ID Middleware", |_, res| {
            Box::pin(async move {
                res.set_raw_header("Server", "battlesnake/github/starter-snake-rust");
            })
        }))
        .attach(Cors)
        .manage(shared_stats)
        .mount(
            "/",
            routes![
                handle_index,
                handle_start,
                handle_move,
                handle_end,
                handle_stats
            ],
        )
}
