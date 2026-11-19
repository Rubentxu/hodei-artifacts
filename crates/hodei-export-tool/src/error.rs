//! Why an export refuses to finish.
//!
//! Every variant names its culprit -- the row, the batch file or the
//! table -- because an operator running a migration probe should not have
//! to guess where the damage is. Corruption is never swallowed: a source
//! that changed under the export, a row that lost a field, a batch file
//! that does not match its recorded hash all fail the run.

/// Why the export did not complete.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The request or the output directory is not usable.
    #[error("{0}")]
    Invalid(String),
    /// The database refused a query.
    #[error("the database failed: {0}")]
    Db(String),
    /// The file system failed.
    #[error("the file system failed: {0}")]
    Io(String),
    /// A row lost a field the export needs: it is truncated or malformed.
    #[error("the row {id} of {table} is truncated or malformed: {detail}")]
    RowDecode {
        /// The table the row belongs to.
        table: String,
        /// The record id of the row, as the source wrote it.
        id: String,
        /// What the row is missing or why it could not be read.
        detail: String,
    },
    /// Two rows of one batch share the same canonical identity: the
    /// uniqueness the source schema declares is broken.
    #[error("the table {table} repeats the canonical identity {key}")]
    DuplicateRow {
        /// The table with the repeated identity.
        table: String,
        /// The repeated canonical identity, fields joined with `|`.
        key: String,
    },
    /// The source moved during the export: the frontier captured before
    /// does not match the one captured after (or before a resume).
    #[error("the source changed during the export: {0}")]
    SourceMutated(String),
    /// A recorded batch file does not hash to the value the progress
    /// marker (or the manifest) recorded for it.
    #[error(
        "the batch file {file} is truncated or altered: expected sha256 {expected}, found {actual}"
    )]
    TruncatedBatch {
        /// The file, relative to the export directory.
        file: String,
        /// The hash that was recorded when the batch was written.
        expected: String,
        /// The hash of the bytes on disk now.
        actual: String,
    },
    /// The resume state on disk is not coherent with this run.
    #[error("the resume state is not usable: {0}")]
    ResumeMismatch(String),
}

impl From<std::io::Error> for ExportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for ExportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Invalid(format!("the JSON serialization failed: {error}"))
    }
}
