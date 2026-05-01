//! Token account validation op.
//!
//! The `Op` struct is retained for reference but dispatch now goes through
//! capability traits (`TokenCheck`, `TokenInitContributor`).

use quasar_lang::prelude::*;

/// Token validation op struct. The derive no longer dispatches through this
/// directly — it uses capability traits. Retained for backward compatibility.
pub struct Op<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}
