//! Built-in, dependency-free Web dashboard.
//!
//! Assets are embedded at compile time so `clusterscope-server` remains one
//! portable binary and the UI is always served from the same origin as its API.

use axum::{
    Router,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::AppState;
use std::sync::Arc;

const INDEX: &str = include_str!("../../../web/index.html");
const STYLES: &str = include_str!("../../../web/styles.css");
const APP: &str = include_str!("../../../web/app.js");
const API: &str = include_str!("../../../web/api.js");
const LIB: &str = include_str!("../../../web/lib.js");
const MOTION: &str = include_str!("../../../web/motion.js");
const ICON: &[u8] = include_bytes!("../../../assets/icon.png");

macro_rules! asset_handler {
    ($name:ident, $body:expr, $content_type:expr) => {
        async fn $name() -> Response {
            asset_response($body, $content_type)
        }
    };
}

asset_handler!(index, INDEX, "text/html; charset=utf-8");
asset_handler!(styles, STYLES, "text/css; charset=utf-8");
asset_handler!(app, APP, "text/javascript; charset=utf-8");
asset_handler!(api, API, "text/javascript; charset=utf-8");
asset_handler!(lib, LIB, "text/javascript; charset=utf-8");
asset_handler!(motion, MOTION, "text/javascript; charset=utf-8");

async fn icon() -> Response {
    with_asset_headers(ICON.into_response(), "image/png")
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/styles.css", get(styles))
        .route("/app.js", get(app))
        .route("/api.js", get(api))
        .route("/lib.js", get(lib))
        .route("/motion.js", get(motion))
        .route("/icon.png", get(icon))
}

fn asset_response(body: &'static str, content_type: &'static str) -> Response {
    with_asset_headers(body.into_response(), content_type)
}

fn with_asset_headers(mut response: Response, content_type: &'static str) -> Response {
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ws: wss:; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn embedded_assets_have_mime_and_security_headers() {
        let response = asset_response(INDEX, "text/html; charset=utf-8");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        assert!(
            response
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY)
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
        assert!(INDEX.contains("id=\"app-view\""));
        assert!(INDEX.contains("src=\"/icon.png\""));
        assert_eq!(&ICON[..8], b"\x89PNG\r\n\x1a\n");
        assert!(!INDEX.contains("job-dialog"));
    }
}
