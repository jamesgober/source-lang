//! # source_lang
//!
//! The multi-file coordinate layer of a compiler front-end. It holds many
//! sources — files and in-memory buffers — in one [`SourceMap`], gives each a
//! stable [`SourceId`], lays them out across a single global position space, and
//! resolves any global [`BytePos`] back to the source and local offset it came
//! from.
//!
//! It is the layer above [`span_lang`]: a [`Span`] says *where in a buffer* an
//! error is, and this crate says *which buffer*, so a diagnostic can name the
//! file as well as the position. It owns source storage and coordinate mapping
//! only — no lexing, no diagnostic rendering.
//!
//! ## Model
//!
//! Sources are placed end to end in the order they are added. The first occupies
//! global offsets `0..len₀`, the next `len₀..len₀ + len₁`, and so on, so the
//! ranges never overlap and the whole project shares one position space. Because
//! each base is the running total, the sources stay sorted by offset and
//! [`SourceMap::locate`] is a binary search — `O(log files)` — that borrows the
//! resolved source rather than copying it. The space is 32 bits wide, so the
//! combined length of every source is capped at `u32::MAX`; overrunning it is a
//! defined [`SourceMapError`], never a silent wrap.
//!
//! ## Quickstart
//!
//! ```
//! use source_lang::{BytePos, SourceMap};
//!
//! let mut map = SourceMap::new();
//! let main = map.add("main.rs", "fn main() {}")?; // global 0..12
//! let util = map.add("util.rs", "fn helper() {}")?; // global 12..26
//!
//! // Resolve a global position to its file and the local offset within it.
//! let (id, local) = map.locate(BytePos::new(13)).expect("inside util.rs");
//! assert_eq!(id, util);
//! assert_eq!(local, BytePos::new(1)); // 13 - 12
//!
//! // The id is a stable handle back to the source.
//! assert_eq!(map.source(main).unwrap().name(), "main.rs");
//! # Ok::<(), source_lang::SourceMapError>(())
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod file;
mod id;
mod map;

pub use error::SourceMapError;
pub use file::SourceFile;
pub use id::SourceId;
pub use map::SourceMap;

// Re-exported so a downstream consuming this crate's API does not also have to
// name `span-lang` as a dependency just to spell the position types it returns.
pub use span_lang::{BytePos, Span};
