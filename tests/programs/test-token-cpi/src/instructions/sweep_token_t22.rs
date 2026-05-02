use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{
        Mint2022, Token2022, Token2022Program,
    },
};

#[derive(Accounts)]
pub struct SweepTokenT22 {
    pub authority: Signer,
    #[account(
        mut,
        token(mint = mint, authority = authority),
        sweep(receiver = receiver, mint = mint, authority = authority)
    )]
    pub source: Account<Token2022>,
    #[account(mut)]
    pub receiver: Account<Token2022>,
    pub mint: Account<Mint2022>,
    pub token_program: Program<Token2022Program>,
}

impl SweepTokenT22 {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
