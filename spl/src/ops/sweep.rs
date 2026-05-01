//! Token sweep — transfer all tokens out before closing.
//!
//! The derive emits direct `TokenSweep::sweep(...)` calls in the epilogue.

use quasar_lang::prelude::*;

/// Trait for token account types that support sweep (transfer all tokens out).
pub trait TokenSweep {
    fn sweep(
        view: &AccountView,
        receiver: &AccountView,
        mint: &AccountView,
        authority: &AccountView,
        token_program: &AccountView,
    ) -> Result<(), ProgramError>;
}
