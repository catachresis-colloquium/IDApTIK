//! Table definitions and the schema tag written on first open.

use redb::TableDefinition;

/// Store-wide metadata, currently just the schema tag.
pub const META: TableDefinition<&str, &[u8]> = TableDefinition::new("idaptik-meta");

/// One [`crate::SessionRecord`] per session, JSON-encoded.
pub const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("idaptik-sessions");

/// The command log. Key is `(session id, seq)`.
///
/// `seq` is store-assigned and strictly increasing; `tick` and `epoch` ride in
/// the value. See the crate docs for why the key is not the tick.
pub const COMMANDS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("idaptik-commands");

/// Checkpoints, keyed the same way. A checkpoint is derived state: deleting
/// every row here loses nothing that the log cannot rebuild.
pub const CHECKPOINTS: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("idaptik-checkpoints");

/// Scenario definitions, keyed by FNV-1a 64 digest of their bytes.
pub const BLOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("idaptik-blobs");

/// The `META` key holding the schema tag.
pub const SCHEMA_KEY: &str = "schema";

/// Every table this build owns, for the open-time existence sweep.
pub const TABLE_NAMES: [&str; 5] = [
    "idaptik-meta",
    "idaptik-sessions",
    "idaptik-commands",
    "idaptik-checkpoints",
    "idaptik-blobs",
];
