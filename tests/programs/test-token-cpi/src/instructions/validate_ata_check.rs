use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, Token, TokenProgram},
};

#[derive(Accounts)]
pub struct ValidateAtaCheck {
    #[account(associated_token(mint = mint, authority = wallet))]
    pub ata: Account<Token>,
    pub mint: Account<Mint>,
    pub wallet: Signer,
    pub token_program: Program<TokenProgram>,
}

impl ValidateAtaCheck {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
