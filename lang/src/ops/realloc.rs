//! Realloc op.
//!
//! Resizes an account's data region after load has validated owner and
//! discriminator. Rejects shrinking below the account type's minimum Space.

use {
    super::{OpCtx, RentAccess, SupportsRealloc},
    crate::account_load::AccountLoad,
    solana_account_view::AccountView,
    solana_program_error::ProgramError,
};

/// Realloc op. Constructed by the derive from `realloc(...)` syntax.
pub struct Op<'a> {
    pub space: usize,
    pub payer: &'a AccountView,
}

impl<'a> Op<'a> {
    /// Apply realloc to a field.
    #[inline(always)]
    pub fn apply<F, R>(&self, field: &mut F, ctx: &OpCtx<'_, R>) -> Result<(), ProgramError>
    where
        F: AccountLoad + SupportsRealloc + crate::traits::Space,
        R: RentAccess,
    {
        let min_space = <F as crate::traits::Space>::SPACE;
        if self.space < min_space {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let view = unsafe { <F as AccountLoad>::to_account_view_mut(field) };
        crate::accounts::realloc_account(view, self.space, self.payer, Some(ctx.rent.get()?))
    }
}
