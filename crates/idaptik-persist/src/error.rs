//! Errors raised by the persistence layer.
//!
//! Hand-written rather than derived: `thiserror` would be a dependency for
//! four `Display` arms and a `From` impl, and this crate already has as many
//! dependencies as it needs.

use crate::record::SessionId;
use std::fmt;

/// Anything the store can refuse or fail to do.
#[derive(Debug)]
pub enum PersistError {
    /// A filesystem operation failed.
    Io(std::io::Error),
    /// The storage engine refused an operation.
    Backend(String),
    /// A record could not be encoded.
    Encode(serde_json::Error),
    /// A record could not be decoded — most often because it was written by a
    /// different schema, or because the bytes were corrupted.
    Decode(serde_json::Error),
    /// The store on disk uses a schema this build does not implement.
    SchemaMismatch { found: u32, expected: u32 },
    /// No session with that id exists.
    UnknownSession(SessionId),
    /// A session with that id already exists; ids are not overwritten.
    SessionExists(SessionId),
    /// This build has no typed resume payload for that simulation yet. The
    /// session can still be written, listed and verified.
    UnsupportedSimKind(crate::record::SimKind),
    /// The session exists but has no checkpoint, so there is nothing to resume.
    NoCheckpoint(SessionId),
    /// A snapshot payload was not the JSON object this crate expects.
    NotAnObject,
    /// A checkpoint names a definition blob that the store does not contain.
    MissingDefinition(String),
    /// A definition blob's bytes do not hash to the digest it was stored under.
    DigestMismatch { expected: String, found: String },
    /// A command batch was appended at a sequence number at or below the one
    /// already recorded. The log is append-only; this is a caller bug.
    SeqRegression { last: u64, attempted: u64 },
    /// A snapshot's own format tag is not the one this build can restore.
    FormatMismatch { found: String, expected: String },
    /// A stored snapshot could not be rebuilt into a running simulation.
    Restore(String),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Backend(m) => write!(f, "storage engine: {m}"),
            Self::Encode(e) => write!(f, "encode: {e}"),
            Self::Decode(e) => write!(f, "decode: {e}"),
            Self::SchemaMismatch { found, expected } => write!(
                f,
                "store schema is {found}, this build implements {expected}; \
                 refusing to read or write a layout it does not understand"
            ),
            Self::UnknownSession(id) => write!(f, "no such session: {}", id.as_str()),
            Self::SessionExists(id) => write!(f, "session already exists: {}", id.as_str()),
            Self::UnsupportedSimKind(k) => write!(
                f,
                "no typed resume for {k:?} yet: the session can be written and \
                 verified, but the payload type arrives with its runtime"
            ),
            Self::NoCheckpoint(id) => {
                write!(f, "session {} has no checkpoint to resume", id.as_str())
            }
            Self::NotAnObject => write!(f, "snapshot payload is not a JSON object"),
            Self::MissingDefinition(d) => write!(f, "definition blob {d} is missing"),
            Self::DigestMismatch { expected, found } => {
                write!(
                    f,
                    "definition digest mismatch: stored {expected}, computed {found}"
                )
            }
            Self::SeqRegression { last, attempted } => write!(
                f,
                "append at seq {attempted} but the log already ends at {last}; \
                 seq must strictly increase"
            ),
            Self::FormatMismatch { found, expected } => write!(
                f,
                "snapshot format {found:?} cannot be restored by a build expecting {expected:?}"
            ),
            Self::Restore(m) => write!(f, "restore: {m}"),
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Encode(e) | Self::Decode(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PersistError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(e: serde_json::Error) -> Self {
        Self::Decode(e)
    }
}

// redb has one umbrella `Error` that every specific error converts into, but
// `?` will not take that two-step conversion, so each type gets an arm. The
// messages are preserved verbatim: redb's diagnostics name the table and the
// key type, and losing them would make a corrupt store much harder to read.
macro_rules! backend_errors {
    ($($t:ty),* $(,)?) => {
        $(impl From<$t> for PersistError {
            fn from(e: $t) -> Self {
                Self::Backend(e.to_string())
            }
        })*
    };
}

backend_errors!(
    redb::Error,
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
);
