use {quasar_lang::prelude::*, quasar_metadata::prelude::*};

/// Validate master edition account with behavior: PDA check.
#[derive(Accounts)]
pub struct ValidateMasterEditionCheck {
    pub metadata_program: Program<MetadataProgram>,
    pub mint: UncheckedAccount,

    #[account(master_edition(program = metadata_program, mint = mint))]
    pub master_edition: Account<MasterEditionAccount>,
}

impl ValidateMasterEditionCheck {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
