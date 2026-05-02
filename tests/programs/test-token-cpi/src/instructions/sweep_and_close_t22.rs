use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{
        Mint2022, Token2022, Token2022Program,
    },
};

#[derive(Accounts)]
pub struct SweepAndCloseT22 {
    pub authority: Signer,
    #[account(
        mut,
        token(mint = mint, authority = authority),
        sweep(receiver = receiver, mint = mint, authority = authority),
        close(dest = destination, authority = authority)
    )]
    pub source: Account<Token2022>,
    #[account(mut)]
    pub receiver: Account<Token2022>,
    pub mint: Account<Mint2022>,
    #[account(mut)]
    pub destination: UncheckedAccount,
    pub token_program: Program<Token2022Program>,
}

impl SweepAndCloseT22 {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
