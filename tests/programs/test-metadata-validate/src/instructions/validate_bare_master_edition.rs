use {quasar_lang::prelude::*, quasar_metadata::prelude::*};

/// Validate bare master edition account (no behavior, just AccountLoad check).
#[derive(Accounts)]
pub struct ValidateBareMasterEdition {
    pub master_edition: Account<MasterEditionAccount>,
}

impl ValidateBareMasterEdition {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
