# RustyFit

RustyFit is a Rust-based web app for preprocessing FIT activity files – clean, validate, merge, and export your fitness data with ease.

This repository currently contains a minimal viable product built with the **Axum** web framework. It provides a simple landing page with a drag-and-drop FIT file uploader and a stub upload endpoint, ready to expand with validation and preprocessing logic.

## Prerequisites
- Rust toolchain (edition 2024)

## Running the server
```bash
cargo run
```
The server listens on `http://0.0.0.0:3000`. Open the address in a browser to see the landing page and try the drag-and-drop uploader.

## Testing
```bash
cargo test
```
The initial tests verify that the landing page responds and that the upload endpoint rejects requests without a file.

## Next steps
- Extend the upload handler to parse and validate FIT files.
- Add persistence for uploaded files and processed results.
- Flesh out the UI with progress indicators and results views.
