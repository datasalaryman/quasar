use {quasar_lang::prelude::*, quasar_metadata::prelude::*};

/// Validate metadata account with behavior + update_authority check.
#[derive(Accounts)]
pub struct ValidateMetadataWithUa {
    pub metadata_program: Program<MetadataProgram>,
    pub mint: UncheckedAccount,
    pub update_authority: Signer,

    #[account(metadata(
        program = metadata_program,
        mint = mint,
        update_authority = update_authority,
    ))]
    pub metadata: Account<MetadataAccount>,
}

impl ValidateMetadataWithUa {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
