use {quasar_lang::prelude::*, quasar_metadata::prelude::*};

/// Validate bare metadata account (no behavior, just AccountLoad check:
/// owner + data_len + discriminator + ZeroPod).
#[derive(Accounts)]
pub struct ValidateBareMetadata {
    pub metadata: Account<MetadataAccount>,
}

impl ValidateBareMetadata {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
