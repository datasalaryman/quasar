use {crate::state::*, quasar_lang::prelude::*};

#[derive(Accounts)]
pub struct MigrateConfig {
    #[account(mut)]
    pub payer: Signer,
    pub system_program: Program<System>,

    #[account(payer = payer, has_one = authority)]
    pub config: Migration<ConfigV1, ConfigV2>,

    /// CHECK: authority validated via has_one
    pub authority: Signer,
}

impl MigrateConfig {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        // Handler reads V1 (source type) via source(). Returns None after finish().
        let _val: u64 = self.config.source().unwrap().value.into();
        Ok(())
    }
}
