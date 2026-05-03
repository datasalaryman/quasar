use {quasar_lang::prelude::*, quasar_metadata::prelude::*};

/// Validate metadata account with behavior: PDA + mint cross-validation.
#[derive(Accounts)]
pub struct ValidateMetadataCheck {
    pub metadata_program: Program<MetadataProgram>,
    pub mint: UncheckedAccount,

    #[account(metadata(program = metadata_program, mint = mint))]
    pub metadata: Account<MetadataAccount>,
}

impl ValidateMetadataCheck {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
