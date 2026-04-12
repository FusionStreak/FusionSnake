//! API-key authentication for protected endpoints.
//!
//! When the `API_KEY` environment variable is set, endpoints that use the
//! [`ApiKey`] extractor require a matching `X-API-Key` header.  If the
//! variable is unset or empty, authentication is disabled (development mode)
//! and a warning is logged on the first request.
//!
//! The comparison uses a constant-time algorithm to prevent timing attacks.

use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest};
use std::future::{Ready, ready};
use std::sync::Once;

static WARN_NO_KEY: Once = Once::new();

/// Constant-time byte comparison to prevent timing side-channels.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Extractor that validates the `X-API-Key` header against the `API_KEY`
/// environment variable.  Add this parameter to any handler that should
/// require authentication.
pub struct ApiKey;

impl FromRequest for ApiKey {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let expected = std::env::var("API_KEY").unwrap_or_default();

        // No key configured → auth is disabled (dev mode)
        if expected.is_empty() {
            WARN_NO_KEY.call_once(|| {
                log::warn!(
                    "API_KEY environment variable is not set — protected endpoints are open. \
                     Set API_KEY to enable authentication."
                );
            });
            return ready(Ok(ApiKey));
        }

        match req.headers().get("X-API-Key") {
            Some(value) if constant_time_eq(value.as_bytes(), expected.as_bytes()) => {
                ready(Ok(ApiKey))
            }
            _ => ready(Err(actix_web::error::ErrorUnauthorized(
                serde_json::json!({"error": "Invalid or missing API key"}),
            ))),
        }
    }
}
