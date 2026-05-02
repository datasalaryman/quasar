use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, Token, TokenProgram},
};
#[derive(Accounts)]
pub struct InitIfNeededToken {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init(idempotent),
        token(mint = mint, authority = payer),
    )]
    pub token_account: Account<Token>,
    pub mint: Account<Mint>,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
}
impl InitIfNeededToken {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
