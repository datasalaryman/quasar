use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, TokenProgram},
};

#[derive(Accounts)]
pub struct ValidateMintCheck {
    #[account(mint(authority = mint_authority, decimals = 6, freeze_authority = None))]
    pub mint: Account<Mint>,
    pub mint_authority: Signer,
    pub token_program: Program<TokenProgram>,
}

impl ValidateMintCheck {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
