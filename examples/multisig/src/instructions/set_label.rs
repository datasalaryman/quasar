use {
    crate::state::MultisigConfig,
    quasar_lang::{prelude::*, sysvars::Sysvar as _},
};

#[derive(Accounts)]
pub struct SetLabel {
    #[account(mut)]
    pub creator: Signer,
    #[account(
        mut,
        has_one = creator,
        seeds = MultisigConfig::seeds(creator),
        bump = config.bump
    )]
    pub config: Account<MultisigConfig>,
    pub system_program: Program<System>,
}

impl SetLabel {
    #[inline(always)]
    pub fn update_label(&mut self, label: &str) -> Result<(), ProgramError> {
        // Snapshot unchanged dynamic fields before taking &mut for the writer.
        // CompactWriter requires all dynamic fields to be set before commit().
        let mut signers_buf = core::mem::MaybeUninit::<[Address; 10]>::uninit();
        let signers = {
            let src = self.config.signers();
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    signers_buf.as_mut_ptr() as *mut Address,
                    src.len(),
                )
            };
            dst.copy_from_slice(src);
            &*dst as &[Address]
        };

        let rent = Rent::get()?;
        let mut writer = self.config.compact_mut(
            self.creator.to_account_view(),
            rent.lamports_per_byte(),
            rent.exemption_threshold_raw(),
        );
        writer.set_label(label)?;
        writer.set_signers(signers)?;
        writer.commit()?;
        Ok(())
    }
}
