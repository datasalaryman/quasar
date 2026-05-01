//! Token close — CPI to the token program.
//!
//! The derive emits direct `TokenClose::close(...)` calls in the epilogue.

use quasar_lang::prelude::*;

/// Trait for token account types that can be closed via CPI.
///
/// Implemented on the behavior target (`Token`, `Token2022`). The close
/// is performed by CPI to the token program, which atomically drains
/// lamports and invalidates the account.
pub trait TokenClose {
    fn close(
        view: &mut AccountView,
        dest: &AccountView,
        authority: &AccountView,
        token_program: &AccountView,
    ) -> Result<(), ProgramError>;
}
