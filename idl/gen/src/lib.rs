//! IDL descriptor IR, resolution pipeline, and codegen model.
//!
//! This crate contains:
//! - Typed descriptor structs (what macros emit)
//! - The resolution pipeline (descriptors → Program IR → canonical IDL JSON)
//! - The normalized codegen model (consumed by client generators)

pub mod diagnostics;
pub mod ir;
pub mod resolve;
