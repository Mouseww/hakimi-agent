use axum::{
    Json, Router,
    extract::{Path, State, Request},
    http::{StatusCode, header},
    routing::{get, post},
    middleware::{self, Next},
    response::Response,
};
async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok());
    let password = std::env::var("HAKIMI_WEBUI_PASSWORD").unwrap_or_else(|_| "password123".to_string());
    if let Some(auth) = auth_header {
        if auth == format!("Bearer {}", password) {
            return Ok(next.run(req).await);
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}
