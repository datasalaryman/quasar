//! Mint account validation op (Phase 3).
//!
//! Validates that a mint account has the expected authority, decimals,
//! freeze authority, and token program. Contributes init params via
//! `apply_init_params`.

use quasar_lang::{
    ops::{AccountOp, OpCtx},
    prelude::*,
};

/// Marker trait for account types with mint account layout.
pub trait HasMintLayout {}

impl HasMintLayout for crate::token::Mint {}
impl HasMintLayout for crate::token_2022::Mint2022 {}

/// Mint validation op. Constructed by the derive from `mint(...)` syntax.
pub struct Op<'a> {
    pub decimals: u8,
    pub authority: &'a AccountView,
    pub freeze_authority: Option<&'a AccountView>,
    pub token_program: &'a AccountView,
}

impl<'a, F: AsAccountView + HasMintLayout> AccountOp<F> for Op<'a> {
    const HAS_AFTER_LOAD: bool = true;
    const HAS_INIT_PARAMS: bool = true;

    #[inline(always)]
    fn after_load(&self, field: &F, _ctx: &OpCtx<'_>) -> Result<(), ProgramError> {
        crate::validate::validate_mint(
            field.to_account_view(),
            self.authority.address(),
            self.decimals,
            self.freeze_authority.map(|fa| fa.address()),
            self.token_program.address(),
        )
    }

    #[inline(always)]
    fn apply_init_params(&self, params: *mut u8) -> Result<(), ProgramError> {
        // SAFETY: For all F: HasMintLayout + AccountInit
        // with InitParams = MintInitParams. The derive passes a properly-typed
        // &mut MintInitParams cast to *mut u8.
        let params: &mut crate::token::MintInitParams<'_> =
            unsafe { &mut *(params as *mut crate::token::MintInitParams<'_>) };
        params.decimals = Some(self.decimals);
        params.authority = Some(self.authority.address());
        params.freeze_authority = self.freeze_authority.map(|fa| fa.address());
        params.token_program = Some(self.token_program);
        Ok(())
    }
}
