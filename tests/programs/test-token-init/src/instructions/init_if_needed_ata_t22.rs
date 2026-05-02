use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{AssociatedTokenProgram, Mint2022, Token2022, Token2022Program},
};
#[derive(Accounts)]
pub struct InitIfNeededAtaT22 {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init(idempotent),
        associated_token(authority = wallet, mint = mint),
    )]
    pub ata: Account<Token2022>,
    pub wallet: Signer,
    pub mint: Account<Mint2022>,
    pub token_program: Program<Token2022Program>,
    pub system_program: Program<SystemProgram>,
    pub ata_program: Program<AssociatedTokenProgram>,
}
impl InitIfNeededAtaT22 {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
