pub mod processing;
pub mod templates;

use axum::{
    Router,
    extract::Multipart,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use processing::{FitProcessError, ProcessingOptions, process_fit_bytes};
use templates::{render_landing_page, render_processed_records};

pub fn build_app() -> Router {
    Router::new()
        .route("/", get(landing_page))
        .route("/upload", post(handle_upload))
}

async fn landing_page() -> Html<String> {
    Html(render_landing_page())
}

async fn handle_upload(mut multipart: Multipart) -> impl IntoResponse {
    let mut uploaded: Option<Vec<u8>> = None;
    let mut options = ProcessingOptions::default();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("file") => match field.bytes().await {
                Ok(bytes) => {
                    uploaded = Some(bytes.to_vec());
                }
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read uploaded file: {err}"),
                    )
                        .into_response();
                }
            },
            Some("remove_speed_fields") => {
                if let Ok(value) = field.text().await {
                    options.remove_speed_fields = value == "true" || value == "on";
                }
            }
            _ => {}
        }
    }

    let file_bytes = match uploaded {
        Some(bytes) => bytes,
        None => return (StatusCode::BAD_REQUEST, "No file provided").into_response(),
    };

    match process_fit_bytes(&file_bytes, &options) {
        Ok(processed) => Html(render_processed_records(&processed)).into_response(),
        Err(err) => render_processing_error(err),
    }
}

fn render_processing_error(error: FitProcessError) -> axum::response::Response {
    (StatusCode::BAD_REQUEST, error.to_string()).into_response()
}
