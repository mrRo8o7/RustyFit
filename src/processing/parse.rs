use crate::processing::types::{FitProcessError, ParsedFit};
use fitparser::FitDataRecord;
use fitparser::de::{DecodeOption, from_bytes_with_options};
use std::collections::HashSet;
use std::convert::TryInto;

/// Parse a raw FIT file into a collection of records while validating CRCs.
///
/// This defers decoding to `fitparser` but keeps the additional header length
/// checks we previously performed to offer clearer error messages when files are
/// truncated or malformed.
pub fn parse_fit(bytes: &[u8]) -> Result<ParsedFit, FitProcessError> {
    let header_size = *bytes
        .first()
        .ok_or_else(|| FitProcessError::InvalidHeader("missing header byte".into()))?
        as usize;

    if header_size < 12 {
        return Err(FitProcessError::InvalidHeader(
            "header too small to be a FIT file".into(),
        ));
    }

    if bytes.len() < header_size + 2 {
        return Err(FitProcessError::InvalidHeader(
            "file shorter than minimum header + CRC".into(),
        ));
    }

    let data_size_start = 4;
    let data_size_end = data_size_start + 4;
    if data_size_end > header_size {
        return Err(FitProcessError::InvalidHeader(
            "header missing data size field".into(),
        ));
    }

    let data_size_bytes = &bytes[data_size_start..data_size_end];
    let data_size = u32::from_le_bytes(data_size_bytes.try_into().unwrap_or_default()) as usize;
    let data_end = header_size + data_size;
    if data_end + 2 > bytes.len() {
        return Err(FitProcessError::InvalidHeader(
            "file shorter than declared data size".into(),
        ));
    }

    let decode_options: HashSet<DecodeOption> = HashSet::new();

    let records: Vec<FitDataRecord> = from_bytes_with_options(bytes, &decode_options)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;

    Ok(ParsedFit { records })
}
