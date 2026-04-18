use axum::body::Body;
use axum::http::header;
use axum::http::{HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web"]
struct WebAssets;

fn content_type_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn resolve_asset_path(uri_path: &str) -> String {
    let trimmed = uri_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "index.html".into();
    }
    if trimmed.ends_with('/') {
        return format!("{trimmed}index.html");
    }
    trimmed.into()
}

/// Embedded concise rules under `web/rules/` (`RustEmbed` folder `web`).
/// `lang`: `None` or `"en"` → English; `"zh"` → Chinese. Other values return an error message.
pub fn rules_markdown(lang: Option<&str>) -> Result<String, String> {
    let token = lang.unwrap_or("en").trim();
    let path = match token.to_ascii_lowercase().as_str() {
        "en" => "rules/rules_en.md",
        "zh" => "rules/rules_zh.md",
        _ => {
            return Err(format!(
                "invalid lang {token:?}; allowed values: en, zh (default: en)"
            ));
        }
    };
    let Some(asset) = WebAssets::get(path) else {
        return Err(format!("missing embedded rules: {path}"));
    };
    String::from_utf8(asset.data.into_owned()).map_err(|e| {
        format!(
            "rules markdown is not utf-8: {}",
            e.utf8_error()
        )
    })
}

pub async fn serve_embedded(uri: Uri) -> Response {
    let path = resolve_asset_path(uri.path());
    let Some(asset) = WebAssets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut resp = Response::new(Body::from(asset.data.into_owned()));
    *resp.status_mut() = StatusCode::OK;
    let content_type = content_type_for_path(&path);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    resp
}
