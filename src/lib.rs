pub mod processing;
pub mod templates;

use axum::{
    Router,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use processing::{FitProcessError, ProcessingOptions, process_fit_bytes};
use std::{collections::HashMap, sync::Arc};
use templates::{render_landing_page, render_processed_records};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Default)]
struct AppState {
    downloads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl AppState {
    async fn insert_download(&self, bytes: Vec<u8>) -> String {
        let id = Uuid::new_v4().to_string();
        self.downloads.lock().await.insert(id.clone(), bytes);
        id
    }

    async fn take_download(&self, id: &str) -> Option<Vec<u8>> {
        self.downloads.lock().await.remove(id)
    }
}

pub fn build_app() -> Router {
    router_with_state(AppState::default())
}

fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/", get(landing_page))
        .route("/upload", post(handle_upload))
        .route("/download/:id", get(download_processed))
        .with_state(state)
}

async fn landing_page() -> Html<String> {
    Html(render_landing_page())
}

async fn handle_upload(State(state): State<AppState>, mut multipart: Multipart) -> impl IntoResponse {
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
            Some("smooth_speed") => {
                if let Ok(value) = field.text().await {
                    options.smooth_speed = value == "true" || value == "on";
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
        Ok(processed) => {
            let download_id = state
                .insert_download(processed.processed_bytes.clone())
                .await;
            let download_url = format!("/download/{download_id}");
            Html(render_processed_records(&processed, &download_url)).into_response()
        }
        Err(err) => render_processing_error(err),
    }
}

fn render_processing_error(error: FitProcessError) -> axum::response::Response {
    (StatusCode::BAD_REQUEST, error.to_string()).into_response()
}

async fn download_processed(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.take_download(&id).await {
        Some(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"processed.fit\""),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn landing_page_responds() {
        let app = build_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn upload_without_file_is_rejected() {
        let app = build_app();
        let req = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "multipart/form-data; boundary=--boundary")
            .body(Body::from("----boundary--"))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn processed_download_can_be_retrieved() {
        let state = AppState::default();
        let app = router_with_state(state.clone());

        let download_id = state.insert_download(vec![1, 2, 3]).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/download/{download_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let collected = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(collected.as_ref(), &[1, 2, 3]);
    }
}
