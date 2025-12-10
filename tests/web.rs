use axum::{body::Body, http::Request, http::StatusCode};
use rustyfit::build_app;
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
