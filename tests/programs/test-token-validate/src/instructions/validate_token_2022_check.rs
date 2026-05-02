use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint2022, Token2022, Token2022Program},
};

#[derive(Accounts)]
pub struct ValidateToken2022Check {
    #[account(token(mint = mint, authority = authority))]
    pub token_account: Account<Token2022>,
    pub mint: Account<Mint2022>,
    pub authority: Signer,
    pub token_program: Program<Token2022Program>,
}

impl ValidateToken2022Check {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
