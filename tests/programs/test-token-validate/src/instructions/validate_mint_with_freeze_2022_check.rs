use {quasar_derive::Accounts, quasar_lang::prelude::*, quasar_spl::prelude::*};

#[derive(Accounts)]
pub struct ValidateMintWithFreeze2022Check {
    #[account(mint(authority = mint_authority, decimals = 6, freeze_authority = Some(freeze_authority), token_program = token_program))]
    pub mint: Account<Mint2022>,
    pub mint_authority: Signer,
    pub freeze_authority: UncheckedAccount,
    pub token_program: Program<Token2022Program>,
}

impl ValidateMintWithFreeze2022Check {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
