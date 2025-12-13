use crate::processing::types::FitProcessError;
use fitparser::{FitDataRecord, from_reader};
use std::io::Cursor;

/// Parse a raw FIT file into a collection of records while validating CRCs
/// using `fitparser`'s reader API.
pub fn parse_fit(bytes: &[u8]) -> Result<Vec<FitDataRecord>, FitProcessError> {
    let mut cursor = Cursor::new(bytes);
    from_reader(&mut cursor).map_err(|err| FitProcessError::ParseError(err.to_string()))
}
