//! Session persistence — JSONL primary + `SQLite` index (Tasks 15.1–15.5).
//!
//! The dual-layer design (see Open Question #4 in `design.md`) keeps two
//! stores side by side:
//!
//! | Layer        | File                              | Holds                          |
//! |--------------|-----------------------------------|--------------------------------|
//! | JSONL log    | `.agent/sessions/<id>.jsonl`      | full append-only entry stream  |
//! | `SQLite` index | `.agent/session-index.sqlite`     | per-session metadata for lists |
//! | Cold Store   | `.agent/cold-store.sqlite`        | cross-session recall archive   |
//!
//! JSONL is human-inspectable, git-diffable, and trivially append-only.
//! `SQLite` indexes the metadata so a "resume" menu or cross-session `recall`
//! doesn't have to parse every JSONL file. The Cold Store is the long-term
//! memory: items archived from one session remain recallable from later
//! sessions.
//!
//! # Modules
//!
//! - [`format`] — [`SessionEntry`] / [`EntryKind`] record vocabulary.
//! - [`writer`] — [`SessionWriter`] append-only JSONL emitter.
//! - [`reader`] — [`SessionReader`] log reloader.
//! - [`index`]  — [`SessionIndex`] `SQLite` metadata index.
//! - [`cold_store`] — cross-session persistent [`crate::memory::Store`].

pub mod cold_store;
pub mod format;
pub mod index;
pub mod reader;
pub mod writer;

pub use cold_store::{COLD_STORE_FILE, cold_store_path, load_cold_store};
pub use format::{EntryKind, SessionEntry, Timestamp};
pub use index::{SESSION_INDEX_FILE, SessionIndex, SessionMeta, session_index_path};
pub use reader::SessionReader;
pub use writer::{SESSIONS_DIR, SessionWriter};
