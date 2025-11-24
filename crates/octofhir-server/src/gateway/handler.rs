//! Custom handler registry for extensible gateway operations.
//!
//! This module provides a trait-based system for registering custom
//! handlers that can be invoked from CustomOperation resources.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    body::Body,
    http::Request,
    response::Response,
};

use super::error::GatewayError;
use super::types::CustomOperation;
use crate::server::AppState;

/// Type alias for handler functions.
///
/// Handlers are async functions that take:
/// - AppState reference
/// - CustomOperation reference
/// - HTTP Request
///
/// And return a Result with Response or GatewayError.
pub type HandlerFn = Arc<
    dyn Fn(
            Arc<AppState>,
            CustomOperation,
            Request<Body>,
        ) -> Pin<Box<dyn Future<Output = Result<Response, GatewayError>> + Send>>
        + Send
        + Sync,
>;

/// Registry for custom gateway handlers.
///
/// This allows developers to register custom Rust handlers that can be
/// invoked from CustomOperation resources with type="handler".
#[derive(Clone)]
pub struct HandlerRegistry {
    handlers: Arc<HashMap<String, HandlerFn>>,
}

impl HandlerRegistry {
    /// Creates a new empty handler registry.
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(HashMap::new()),
        }
    }

    /// Registers a new handler with the given name.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut registry = HandlerRegistry::new();
    /// registry.register("my_handler", Arc::new(|state, op, req| {
    ///     Box::pin(async move {
    ///         // Handle the request
    ///         Ok(Response::new("Hello from custom handler".into()))
    ///     })
    /// }));
    /// ```
    pub fn register(&mut self, name: impl Into<String>, handler: HandlerFn) {
        Arc::get_mut(&mut self.handlers)
            .expect("Cannot register handler after registry is shared")
            .insert(name.into(), handler);
    }

    /// Looks up a handler by name.
    pub fn get(&self, name: &str) -> Option<&HandlerFn> {
        self.handlers.get(name)
    }

    /// Returns the number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Checks if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handles custom handler operations by looking up and invoking registered handlers.
///
/// This handler:
/// 1. Extracts handler name from the operation
/// 2. Looks up the handler in the registry
/// 3. Invokes the handler with the request
pub async fn handle_handler(
    state: Arc<AppState>,
    operation: &CustomOperation,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let handler_name = operation.handler.as_ref().ok_or_else(|| {
        GatewayError::InvalidConfig("Handler operation missing handler name".to_string())
    })?;

    tracing::info!(handler_name = %handler_name, "Invoking custom handler");

    // Look up the handler in the registry
    let handler_fn = state
        .handler_registry
        .get(handler_name)
        .ok_or_else(|| GatewayError::HandlerNotFound(format!(
            "Handler '{}' not found in registry",
            handler_name
        )))?;

    // Invoke the handler
    handler_fn(state.clone(), operation.clone(), request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = HandlerRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = HandlerRegistry::new();

        let handler: HandlerFn = Arc::new(|_state, _op, _req| {
            Box::pin(async move {
                Ok(Response::new(Body::empty()))
            })
        });

        registry.register("test_handler", handler);

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.get("test_handler").is_some());
        assert!(registry.get("nonexistent").is_none());
    }
}
