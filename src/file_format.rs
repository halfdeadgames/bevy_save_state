use serde::{Deserialize, Serialize};

use crate::error::SaveStateError;

/// Version of the savestate file layout. Bump it when the layout itself changes.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// First line of every savestate file, written as one line of RON.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SaveHeader {
    /// File layout version, compared with [`CURRENT_FORMAT_VERSION`] on load.
    pub format_version: u32,
    /// Hash of every saved type and its fields, compared with the current build on load.
    pub schema_fingerprint: u64,
}

impl SaveHeader {
    /// Refuses a header written by an incompatible format or a different set of saved types.
    ///
    /// # Errors
    /// Returns `FormatVersionMismatch` or `SchemaMismatch`.
    pub fn verify_against(&self, current_fingerprint: u64) -> Result<(), SaveStateError> {
        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(SaveStateError::FormatVersionMismatch {
                saved: self.format_version,
                current: CURRENT_FORMAT_VERSION,
            });
        }
        if self.schema_fingerprint != current_fingerprint {
            return Err(SaveStateError::SchemaMismatch {
                saved: self.schema_fingerprint,
                current: current_fingerprint,
            });
        }
        Ok(())
    }
}

/// Builds the file text: the header on one line, a newline, then the world section.
///
/// # Errors
/// Returns `SerializeHeader` if the header cannot be written.
pub fn join_header_and_world(
    header: &SaveHeader,
    world_text: &str,
) -> Result<String, SaveStateError> {
    let header_text = ron::ser::to_string(header)
        .map_err(|error| SaveStateError::SerializeHeader(error.to_string()))?;
    Ok(format!("{header_text}\n{world_text}"))
}

/// Splits file text into its parsed header and the unparsed world section.
///
/// # Errors
/// Returns `MalformedFile` without a newline, or `ParseHeader` if the first line is not a header.
pub fn split_header_and_world(text: &str) -> Result<(SaveHeader, &str), SaveStateError> {
    let (header_text, world_text) = text.split_once('\n').ok_or(SaveStateError::MalformedFile {
        reason: "expected a header line followed by a world section",
    })?;
    let header = ron::de::from_str::<SaveHeader>(header_text)
        .map_err(|error| SaveStateError::ParseHeader(error.to_string()))?;
    Ok((header, world_text))
}
