//! Init op: Phase 1 account initialization via system program CPI.
//!
//! `init::Op` calls `AccountInit::init` on the field's behavior target when
//! the account is owned by the system program (uninitialized). When
//! `idempotent = true`, already-initialized accounts are silently accepted.

use {
    super::OpCtx,
    crate::{
        account_init::{AccountInit, InitCtx},
        account_load::AccountLoad,
        cpi::Signer,
    },
    solana_account_view::AccountView,
    solana_program_error::ProgramError,
};

/// Init operation. Constructed by the derive macro from `init(...)` syntax.
///
/// Generic `Params` defaults to `()` for plain `#[account]` types.
/// Init contributors (token, mint, ata_init) populate params via capability
/// traits before this op runs.
pub struct Op<'a, Params = ()> {
    pub payer: &'a AccountView,
    pub space: u64,
    pub signers: &'a [Signer<'a, 'a>],
    pub params: Params,
    pub idempotent: bool,
}

impl<'a, P> Op<'a, P> {
    /// Execute the init operation on a raw account slot.
    #[inline(always)]
    pub fn apply<F: AccountLoad + AccountInit<InitParams<'a> = P>>(
        &self,
        slot: &mut AccountView,
        ctx: &OpCtx<'_>,
    ) -> Result<(), ProgramError> {
        if crate::is_system_program(slot.owner()) {
            let target = unsafe { &mut *(slot as *mut AccountView) };
            let program_id = unsafe { &*(ctx.program_id as *const solana_address::Address) };
            let rent = ctx.rent()?;
            let rent = unsafe { &*(rent as *const crate::sysvars::rent::Rent) };
            <F as AccountInit>::init(
                InitCtx {
                    payer: self.payer,
                    target,
                    program_id,
                    space: self.space,
                    signers: self.signers,
                    rent,
                },
                &self.params,
            )?;
        } else if !self.idempotent {
            return Err(ProgramError::AccountAlreadyInitialized);
        }
        Ok(())
    }
}
