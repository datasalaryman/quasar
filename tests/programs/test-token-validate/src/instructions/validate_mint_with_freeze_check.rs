use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, TokenProgram},
};

#[derive(Accounts)]
pub struct ValidateMintWithFreezeCheck {
    #[account(mint(authority = mint_authority, decimals = 6, freeze_authority = Some(freeze_authority)))]
    pub mint: Account<Mint>,
    pub mint_authority: Signer,
    pub freeze_authority: UncheckedAccount,
    pub token_program: Program<TokenProgram>,
}

impl ValidateMintWithFreezeCheck {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
