//! `LoggedJson` extractor — logs the raw request body for every game endpoint.
//!
//! At DEBUG level: always logs the path + byte count + raw payload.
//! At WARN level:  logs the full payload when JSON deserialization fails.

use futures_util::future::LocalBoxFuture;
use log::debug;

pub struct LoggedJson<T>(pub T);

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
