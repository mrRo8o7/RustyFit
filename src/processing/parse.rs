use crate::processing::types::{FitProcessError, ParsedFit};
use fitparser::FitDataRecord;
use fitparser::de::{DecodeOption, from_bytes_with_options};
use std::collections::HashSet;
use std::convert::TryInto;

/// Parse a raw FIT file into its component parts while validating CRCs.
///
/// The official FIT structure is enforced here: the header length must be at
/// least 12 bytes, the declared data length must match the payload present, and
/// the file must be long enough to include the final two-byte CRC. CRC values
/// are verified so corruption can be reported back to the caller.
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

    let has_header_crc = header_size > 12;
    let header_without_crc_end = if has_header_crc {
        header_size - 2
    } else {
        header_size
    };
    let header_without_crc = bytes[..header_without_crc_end].to_vec();

    let data_size_start = 4;
    let data_size_end = data_size_start + 4;
    if data_size_end > header_without_crc.len() {
        return Err(FitProcessError::InvalidHeader(
            "header missing data size field".into(),
        ));
    }

    let data_size = u32::from_le_bytes(
        header_without_crc[data_size_start..data_size_end]
            .try_into()
            .map_err(|_| FitProcessError::InvalidHeader("unable to read data size".into()))?,
    ) as usize;

    let data_start = header_size;
    let data_end = data_start + data_size;
    if data_end + 2 > bytes.len() {
        return Err(FitProcessError::InvalidHeader(
            "file shorter than declared data size".into(),
        ));
    }

    let data_section = bytes[data_start..data_end].to_vec();

    let decode_options: HashSet<DecodeOption> = HashSet::new();

    let records: Vec<FitDataRecord> = from_bytes_with_options(bytes, &decode_options)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;

    Ok(ParsedFit {
        header_without_crc,
        has_header_crc,
        data_section,
        records,
    })
}
