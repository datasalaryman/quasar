use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint, TokenProgram},
};
#[derive(Accounts)]
pub struct InitIfNeededMint {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init(idempotent),
        mint(decimals = 6, authority = mint_authority, freeze_authority = None),
    )]
    pub mint: Account<Mint>,
    pub mint_authority: Signer,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
}
impl InitIfNeededMint {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
