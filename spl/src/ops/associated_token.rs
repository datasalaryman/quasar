//! Associated Token Account op — validate-only.
//!
//! Dispatch now goes through `AtaCheck` capability trait.

use quasar_lang::prelude::*;

/// ATA validate-only op struct. Retained for backward compatibility.
pub struct Op<'a> {
    pub authority: &'a AccountView,
    pub mint: &'a AccountView,
    pub token_program: &'a AccountView,
}
