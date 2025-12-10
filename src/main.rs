use std::net::SocketAddr;

use axum::{
    Router,
    extract::Multipart,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn build_app() -> Router {
    Router::new()
        .route("/", get(landing_page))
        .route("/upload", post(handle_upload))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustyfit=debug,tower_http=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = build_app();
    let addr: SocketAddr = "0.0.0.0:3000".parse().expect("valid socket address");
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app.into_make_service())
        .await
        .expect("server crashed");
}

async fn landing_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>RustyFit</title>
  <style>
    body { font-family: Arial, sans-serif; margin: 0; padding: 0; background: #f7f7f7; }
    header { background: #20232a; color: white; padding: 1rem 2rem; }
    main { padding: 2rem; max-width: 960px; margin: 0 auto; }
    .drop-zone { border: 2px dashed #888; padding: 2rem; background: white; text-align: center; }
    .drop-zone.drag { border-color: #2563eb; background: #eff6ff; }
    .status { margin-top: 1rem; }
    button { background: #2563eb; color: white; border: none; padding: 0.75rem 1.5rem; border-radius: 4px; cursor: pointer; }
    button:hover { background: #1d4ed8; }
  </style>
</head>
<body>
  <header><h1>RustyFit MVP</h1></header>
  <main>
    <p>Upload a FIT file to begin preprocessing.</p>
    <div id="drop-zone" class="drop-zone">
      <p>Drag & drop your FIT file here, or click to select.</p>
      <input id="file-input" type="file" accept=".fit" style="display:none" />
      <button id="select-btn" type="button">Choose a file</button>
    </div>
    <p class="status" id="status"></p>
  </main>
  <script>
    const dropZone = document.getElementById('drop-zone');
    const fileInput = document.getElementById('file-input');
    const selectBtn = document.getElementById('select-btn');
    const statusEl = document.getElementById('status');

    const preventDefaults = (e) => { e.preventDefault(); e.stopPropagation(); };
    ['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
      dropZone.addEventListener(eventName, preventDefaults, false);
      document.body.addEventListener(eventName, preventDefaults, false);
    });

    ['dragenter', 'dragover'].forEach(eventName => {
      dropZone.addEventListener(eventName, () => dropZone.classList.add('drag'), false);
    });
    ['dragleave', 'drop'].forEach(eventName => {
      dropZone.addEventListener(eventName, () => dropZone.classList.remove('drag'), false);
    });

    dropZone.addEventListener('click', () => fileInput.click());
    selectBtn.addEventListener('click', () => fileInput.click());

    dropZone.addEventListener('drop', handleFiles);
    fileInput.addEventListener('change', (e) => handleFiles({ dataTransfer: { files: e.target.files } }));

    async function handleFiles(e) {
      const files = e.dataTransfer.files;
      if (!files || files.length === 0) {
        return;
      }
      const formData = new FormData();
      formData.append('file', files[0]);
      statusEl.textContent = 'Uploading...';
      try {
        const response = await fetch('/upload', { method: 'POST', body: formData });
        const message = await response.text();
        statusEl.textContent = response.ok ? message : 'Upload failed: ' + message;
      } catch (err) {
        statusEl.textContent = 'Upload failed: ' + err;
      }
    }
  </script>
</body>
</html>"#,
    )
}

async fn handle_upload(mut multipart: Multipart) -> impl IntoResponse {
    let mut file_count = 0u32;

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            file_count += 1;
        }
    }

    match file_count {
        0 => (StatusCode::BAD_REQUEST, "No file provided").into_response(),
        _ => (StatusCode::OK, format!("Received {file_count} file(s).")).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::build_app;
    use axum::http::StatusCode;
    use axum::{body::Body, http::Request};
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
}
