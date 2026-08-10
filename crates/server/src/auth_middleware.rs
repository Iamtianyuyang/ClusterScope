use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use common::auth::Claims;
use jsonwebtoken::decode;
use std::sync::Arc;

pub async fn auth_middleware(
    State(secret): State<Arc<String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Skip JWT validation for public endpoints (checked before token extraction)
    let path = request.uri().path();
    if path == "/api/health" || path == "/api/login" || path == "/api/refresh-token" {
        return Ok(next.run(request).await);
    }

    let Some(token) = extract_token(&request)? else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    match validate_token(&token, &secret) {
        Ok(claims) => {
            request.extensions_mut().insert(claims);
            Ok(next.run(request).await)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Middleware for read-only mode: GET/HEAD pass without a token;
/// mutating requests still require a valid JWT.
pub async fn readonly_middleware(
    State(secret): State<Arc<String>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path();
    if path == "/api/health" || path == "/api/login" || path == "/api/refresh-token" {
        return Ok(next.run(request).await);
    }

    // Read-only requests are public in this mode.
    if request.method() == axum::http::Method::GET
        || request.method() == axum::http::Method::HEAD
    {
        return Ok(next.run(request).await);
    }

    // Mutating requests still need a token.
    let Some(token) = extract_token(&request)? else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    match validate_token(&token, &secret) {
        Ok(claims) => {
            let mut request = request;
            request.extensions_mut().insert(claims);
            Ok(next.run(request).await)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Role gate for admin-only routes (users, alert rules, …). Must run after
/// [`auth_middleware`] so the claims extension is populated.
pub async fn require_admin_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    require_role_middleware(request, next, &["admin"]).await
}

/// Role gate for operator-or-admin routes (job submission, job stop).
pub async fn require_operator_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    require_role_middleware(request, next, &["operator", "admin"]).await
}

async fn require_role_middleware(
    request: Request,
    next: Next,
    allowed: &[&str],
) -> Result<Response, StatusCode> {
    let claims = request
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    crate::handlers::check_role(&claims, allowed)?;
    Ok(next.run(request).await)
}

pub fn extract_token(request: &Request) -> Result<Option<String>, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(Some(auth_header[7..].to_string()))
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )?;

    Ok(data.claims)
}
