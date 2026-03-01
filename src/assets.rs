use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use mime_guess::from_path;
use rust_embed::RustEmbed;

/// All files produced by `npm run build` inside `frontend/` are embedded into
/// the binary.  The path is relative to the workspace root (where Cargo.toml
/// lives), so build the frontend first with:
///   cd frontend && npm ci && npm run build
#[derive(RustEmbed)]
#[folder = "frontend/build/"]
pub struct Assets;

/// Serve an embedded asset by URI path.  Falls back to `index.html` for any
/// path that does not match a real asset – this is essential for client-side
/// SvelteKit routing.
pub async fn static_handler(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');

    // Try exact match first.
    if let Some(content) = Assets::get(raw_path) {
        return serve_asset(raw_path, content.data);
    }

    // If the path has no extension it is likely a client-side route; serve
    // the SPA shell so the router can take over.
    if !raw_path.contains('.') {
        if let Some(index) = Assets::get("index.html") {
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"))],
                Body::from(index.data),
            )
                .into_response();
        }
    }

    (StatusCode::NOT_FOUND, "404 Not Found").into_response()
}

pub fn serve_asset(path: &str, data: std::borrow::Cow<'static, [u8]>) -> Response {
    let mime = from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref())
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        )
        // Cache immutable hashed assets aggressively; HTML must revalidate.
        .header(
            header::CACHE_CONTROL,
            if path.ends_with(".html") {
                HeaderValue::from_static("no-cache")
            } else {
                HeaderValue::from_static("public, max-age=31536000, immutable")
            },
        )
        .body(Body::from(data))
        .unwrap()
}
