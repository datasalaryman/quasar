//! Realloc op (Phase 3b).
//!
//! Resizes an account's data region after load has validated owner and
//! discriminator. Rejects shrinking below the account type's minimum Space.

use {
    super::{OpCtx, SupportsRealloc},
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
    /// Apply realloc to a field. The derive emits
    /// `realloc_op.apply::<#ty>(&mut #ident, &__ctx)?;`
    #[inline(always)]
    pub fn apply<F: AccountLoad + SupportsRealloc + crate::traits::Space>(
        &self,
        field: &mut F,
        ctx: &OpCtx<'_>,
    ) -> Result<(), ProgramError> {
        let min_space = <F as crate::traits::Space>::SPACE;
        if self.space < min_space {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let view = unsafe { <F as AccountLoad>::to_account_view_mut(field) };
        crate::accounts::realloc_account(view, self.space, self.payer, Some(ctx.rent()?))
    }
}
