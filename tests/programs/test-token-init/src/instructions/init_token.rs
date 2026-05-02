use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, Token, TokenProgram},
};
#[derive(Accounts)]
pub struct InitToken {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init,
        token(mint = mint, authority = payer),
    )]
    pub token_account: Account<Token>,
    pub mint: Account<Mint>,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
}
impl InitToken {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
