//! The set of engines served by this process.
//!
//! Each engine is primed once at startup and mounted as its own Battlesnake
//! under `/<engine-name>` (see [`crate::engine_routes`]). To add an engine,
//! add a field here, then include it in [`EngineRegistry::prime`],
//! [`EngineRegistry::names`], [`EngineRegistry::configure`] and
//! [`EngineRegistry::merge_openapi`].

use fusion_core::SnakeEngine;
use fusion_engine_standard::engine::StandardEngine;
use utoipa::openapi::{OpenApi, RefOr};
use utoipa::{PartialSchema, ToSchema};

use crate::engine_routes::configure_engine;

/// Top-level path segments owned by the server; engines may not use them.
const RESERVED_NAMES: &[&str] = &["stats", "training", "api-doc"];

#[derive(Clone)]
pub struct EngineRegistry {
    standard: StandardEngine,
}

impl EngineRegistry {
    /// Prime every engine and validate their route names.
    ///
    /// # Panics
    /// Panics if an engine name is not a valid, unique route segment.
    #[must_use]
    pub fn prime() -> Self {
        let registry = Self {
            standard: <StandardEngine as SnakeEngine>::prime(),
        };
        validate_names(&registry.names());
        registry
    }

    /// Names of all registered engines, in mount order.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        vec![self.standard.name()]
    }

    /// Mount every engine's routes.
    pub fn configure(&self, cfg: &mut actix_web::web::ServiceConfig) {
        configure_engine(cfg, self.standard.clone());
    }

    /// Document the registered engine names and each engine's params schema.
    pub fn merge_openapi(&self, openapi: &mut OpenApi) {
        add_params_schema::<StandardEngine>(openapi);

        let names = self.names().join(", ");
        let engine_params = openapi
            .paths
            .paths
            .values_mut()
            .flat_map(|item| [item.get.as_mut(), item.post.as_mut()])
            .flatten()
            .flat_map(|op| op.parameters.iter_mut().flatten());
        for param in engine_params {
            if let RefOr::T(param) = param
                && param.name == "engine"
            {
                param.description = Some(format!("Engine name. Registered: {names}"));
            }
        }
    }
}

fn add_params_schema<E: SnakeEngine>(openapi: &mut OpenApi) {
    let components = openapi.components.get_or_insert_with(Default::default);
    let mut schemas = vec![(E::Params::name().into_owned(), E::Params::schema())];
    E::Params::schemas(&mut schemas);
    components.schemas.extend(schemas);
}

fn validate_names(names: &[&'static str]) {
    for (i, name) in names.iter().enumerate() {
        assert!(
            !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'_'),
            "Engine name {name:?} must be a non-empty lowercase route segment ([a-z0-9_-])"
        );
        assert!(
            !RESERVED_NAMES.contains(name),
            "Engine name {name:?} collides with a reserved server route"
        );
        assert!(
            !names[..i].contains(name),
            "Engine name {name:?} is registered more than once"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::validate_names;

    #[test]
    fn accepts_valid_unique_names() {
        validate_names(&["standard", "solo-v2", "duel_exp"]);
    }

    #[test]
    #[should_panic(expected = "reserved")]
    fn rejects_reserved_names() {
        validate_names(&["stats"]);
    }

    #[test]
    #[should_panic(expected = "more than once")]
    fn rejects_duplicate_names() {
        validate_names(&["standard", "standard"]);
    }

    #[test]
    #[should_panic(expected = "route segment")]
    fn rejects_non_route_safe_names() {
        validate_names(&["Standard/Exp"]);
    }
}
