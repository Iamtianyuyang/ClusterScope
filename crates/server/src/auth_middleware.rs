use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use common::auth::{Claims, UserRole};
use std::sync::Arc;

use crate::AppState;

/// Re-check that the token's subject still exists, is enabled and not
/// locked. The JWT is a snapshot from login time: a deleted/disabled user
/// would otherwise keep read access until the token expires (role-gated
/// routes already re-check in [`require_role`]; this closes the same gap for
/// every other authenticated route, including the WebSocket upgrade).
pub(crate) async fn user_is_active(state: &AppState, user_id: &str) -> bool {
    match storage::user_queries::get_user_by_id(state.database.pool(), user_id).await {
        Ok(Some(u)) => u.enabled && u.locked_until.map(|t| t <= Utc::now()).unwrap_or(true),
        _ => false,
    }
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
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

    match validate_token(&token, &state.jwt_secret) {
        Ok(claims) => {
            if !user_is_active(&state, &claims.sub).await {
                return Err(StatusCode::UNAUTHORIZED);
            }
            request.extensions_mut().insert(claims);
            Ok(next.run(request).await)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Middleware for read-only mode: GET/HEAD pass without a token;
/// mutating requests still require a valid JWT.
pub async fn readonly_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path();
    if path == "/api/health" || path == "/api/login" || path == "/api/refresh-token" {
        return Ok(next.run(request).await);
    }

    // Read-only requests are public in this mode — but if a non-empty token
    // IS supplied it must be valid, and its claims are attached for role gates.
    if request.method() == axum::http::Method::GET || request.method() == axum::http::Method::HEAD {
        if let Ok(Some(token)) = extract_token(&request) {
            if token.is_empty() {
                // Empty bearer token == no token (e.g. tools that always
                // attach an Authorization header).
                return Ok(next.run(request).await);
            }
            match validate_token(&token, &state.jwt_secret) {
                Ok(claims) => {
                    if !user_is_active(&state, &claims.sub).await {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    request.extensions_mut().insert(claims);
                }
                Err(_) => return Err(StatusCode::UNAUTHORIZED),
            }
        }
        return Ok(next.run(request).await);
    }

    // Mutating requests still need a token.
    let Some(token) = extract_token(&request)? else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    match validate_token(&token, &state.jwt_secret) {
        Ok(claims) => {
            if !user_is_active(&state, &claims.sub).await {
                return Err(StatusCode::UNAUTHORIZED);
            }
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
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    require_role(state, request, next, &["admin"]).await
}

/// Role gate for operator-or-admin routes (job submission, job stop).
pub async fn require_operator_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    require_role(state, request, next, &["operator", "admin"]).await
}

/// Check the role against the CURRENT database row instead of the JWT claim:
/// the token's `role` is a snapshot from login time, so a demoted admin would
/// otherwise keep admin powers until the access token expires. Checking the
/// live row also rejects disabled/locked users and deleted accounts whose
/// tokens are still unexpired.
async fn require_role(
    state: Arc<AppState>,
    request: Request,
    next: Next,
    allowed: &[&str],
) -> Result<Response, StatusCode> {
    let claims = request
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user = storage::user_queries::get_user_by_id(state.database.pool(), &claims.sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !user.enabled {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(locked_until) = user.locked_until
        && locked_until > Utc::now()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // C-API adaptation: this tree implements FromStr for UserRole
    // (B had an inherent from_role_str helper).
    let role = user.role.parse::<UserRole>().unwrap_or(UserRole::Viewer);
    if allowed.contains(&role.to_str()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
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
    // Single implementation in common::auth (leeway 0, `iat` check); map
    // every failure to InvalidToken so callers see one error shape.
    common::auth::verify_jwt(token, secret).map_err(|_| {
        jsonwebtoken::errors::Error::from(jsonwebtoken::errors::ErrorKind::InvalidToken)
    })
}
