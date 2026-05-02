use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{
        Mint, Token, TokenProgram,
    },
};

/// Tests sweep + close -- transfers all tokens, then closes the account.
#[derive(Accounts)]
pub struct SweepAndClose {
    pub authority: Signer,
    #[account(
        mut,
        token(mint = mint, authority = authority),
        sweep(receiver = receiver, mint = mint, authority = authority),
        close(dest = destination, authority = authority)
    )]
    pub source: Account<Token>,
    #[account(mut)]
    pub receiver: Account<Token>,
    pub mint: Account<Mint>,
    #[account(mut)]
    pub destination: UncheckedAccount,
    pub token_program: Program<TokenProgram>,
}

impl SweepAndClose {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
