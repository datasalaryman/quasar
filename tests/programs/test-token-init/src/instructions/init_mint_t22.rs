use {
    quasar_derive::Accounts,
    quasar_lang::prelude::*,
    quasar_spl::{Mint2022, Token2022Program},
};
#[derive(Accounts)]
pub struct InitMintT22 {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init,
        mint(decimals = 6, authority = mint_authority, freeze_authority = None),
    )]
    pub mint: Account<Mint2022>,
    pub mint_authority: Signer,
    pub token_program: Program<Token2022Program>,
    pub system_program: Program<SystemProgram>,
}
impl InitMintT22 {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
