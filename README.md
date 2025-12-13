# RustyFit

RustyFit is a Rust-based web app for preprocessing FIT activity files – clean, validate, merge, and export your fitness data with ease.

The current Axum server lets you upload a FIT file, renders the decoded records in the browser, and (optionally) strips speed-related fields before returning a rebuilt FIT payload.

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

## How FIT files are parsed and rewritten

The FIT protocol stores binary data with a small header, a stream of definition and data messages, and a trailing CRC. RustyFit uses [`fitparser`](https://docs.rs/fitparser/latest/fitparser/) to decode the stream for display, and hand-written utilities in [`src/processing.rs`](src/processing.rs) to keep the on-disk structure valid when fields are removed.

```
+--------------------------- FIT file ----------------------------+
| Header (size byte, data size, profile/version, optional CRC)    |
+-----------------------------------------------------------------+
| Message stream (definition + data messages)                     |
|   ├─ Definition #0: fields + types + base sizes                 |
|   ├─ Data #0 instances, matching Definition #0 layout           |
|   ├─ Definition #1: same structure but with its own field set   |
|   ├─ Data #1 instances, matching Definition #1 layout           |
|   └─ ... (alternating definitions and data records)             |
+-----------------------------------------------------------------+
| File CRC (2 bytes)                                              |
+-----------------------------------------------------------------+
```

1. `parse_fit` enforces basic FIT layout: the first byte declares the header size, the next four bytes declare the data payload length, and the file ends with a two-byte CRC.
2. The parsed `FitDataRecord`s are converted into human-readable `DisplayRecord`s for the UI.
3. Speed filtering and smoothing operate on decoded `FitDataRecord`s so we can drop or adjust fields without manually rewriting FIT headers.
4. The updated records are re-encoded with `fitparser::encode_records`, which rebuilds the FIT header and CRC for us.

Reading through `processing.rs` alongside a FIT specification (or the links below) is the quickest way to understand the project’s handling of the format.

### Additional FIT references
- [FIT SDK documentation](https://developer.garmin.com/fit/protocol/) for the canonical file layout and message types.
