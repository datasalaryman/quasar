//! ATA init op.
//!
//! Dispatch now goes through `AtaCheck` + `AtaInitContributor` capability traits.

use quasar_lang::prelude::*;

/// ATA init op struct. Retained for backward compatibility.
pub struct Op<'a> {
    pub authority: &'a AccountView,
    pub mint: &'a AccountView,
    pub payer: &'a AccountView,
    pub token_program: &'a AccountView,
    pub system_program: &'a AccountView,
    pub ata_program: &'a AccountView,
    pub idempotent: bool,
}
