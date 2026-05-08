use crate::prelude::*;

/// Program interface wrapper. Validates against multiple program IDs via
/// `ProgramInterface`.
#[repr(transparent)]
pub struct Interface<T: ProgramInterface> {
    view: AccountView,
    _marker: core::marker::PhantomData<T>,
}

impl<T: ProgramInterface> AsAccountView for Interface<T> {
    #[inline(always)]
    fn to_account_view(&self) -> &AccountView {
        &self.view
    }
}

impl<T: ProgramInterface> crate::account_load::AccountLoad for Interface<T> {
    const IS_EXECUTABLE: bool = true;

    #[inline(always)]
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        if crate::utils::hint::unlikely(!T::matches(view.address())) {
            #[cfg(feature = "debug")]
            crate::prelude::log(&::alloc::format!(
                "Program interface mismatch: address {} does not match any allowed programs",
                view.address()
            ));
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(())
    }
}

impl<T: ProgramInterface> Interface<T> {
    /// # Safety
    /// Caller must ensure executable flag and address match.
    #[inline(always)]
    pub unsafe fn from_account_view_unchecked(view: &AccountView) -> &Self {
        &*(view as *const AccountView as *const Self)
    }
}
