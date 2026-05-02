use {quasar_derive::Accounts, quasar_lang::prelude::*, quasar_spl::prelude::*};

#[derive(Accounts)]
pub struct ValidateAta2022Check {
    #[account(associated_token(mint = mint, authority = wallet, token_program = token_program))]
    pub ata: Account<Token2022>,
    pub mint: Account<Mint2022>,
    pub wallet: Signer,
    pub token_program: Program<Token2022Program>,
}

impl ValidateAta2022Check {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
