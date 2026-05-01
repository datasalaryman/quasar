//! SPL token operations.
//!
//! Capability traits (`capabilities`) and context structs (`ctx`) are the
//! public dispatch surface. The derive emits direct capability trait calls.

pub mod associated_token;
pub mod ata_init;
pub mod capabilities;
pub mod close;
pub mod ctx;
pub mod mint;
pub mod realloc;
pub mod sweep;
pub mod token;
