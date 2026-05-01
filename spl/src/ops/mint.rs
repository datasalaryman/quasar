//! Mint account validation op.
//!
//! The `Op` struct is retained for reference but dispatch now goes through
//! capability traits (`MintCheck`, `MintInitContributor`).

use quasar_lang::prelude::*;

/// Mint validation op struct. Retained for backward compatibility.
pub struct Op<'a> {
    pub decimals: u8,
    pub authority: &'a AccountView,
    pub freeze_authority: Option<&'a AccountView>,
    pub token_program: &'a AccountView,
}
