use crate::cpi::system::SYSTEM_PROGRAM_ID;
use crate::prelude::*;
use core::marker::PhantomData;

/// Realloc an account to `new_space` bytes, transferring lamports to/from `payer`
/// to maintain rent-exemption. Used by `Account::realloc` and generated View types.
#[inline(always)]
pub fn realloc_account(
    view: &AccountView,
    new_space: usize,
    payer: &AccountView,
    rent: Option<&crate::sysvars::rent::Rent>,
) -> Result<(), ProgramError> {
    let rent_exempt_lamports = match rent {
        Some(rent) => rent.try_minimum_balance(new_space)?,
        None => {
            use crate::sysvars::Sysvar;
            crate::sysvars::rent::Rent::get()?.try_minimum_balance(new_space)?
        }
    };

    let current_lamports = view.lamports();

    if rent_exempt_lamports > current_lamports {
        crate::cpi::system::transfer(payer, view, rent_exempt_lamports - current_lamports)
            .invoke()?;
    } else if current_lamports > rent_exempt_lamports {
        let excess = current_lamports - rent_exempt_lamports;
        view.set_lamports(rent_exempt_lamports);
        payer.set_lamports(payer.lamports() + excess);
    }

    view.resize(new_space)?;
    Ok(())
}

/// Typed account wrapper with composable validation.
///
/// `Account<T>` is the unified wrapper for all validated on-chain accounts.
/// The trait bounds on `T` determine which capabilities are available:
///
/// ## Single-owner accounts (T: Owner)
///
/// ```ignore
/// // Validates owner == SPL Token program
/// pub token: &'info Account<TokenAccount>,
/// ```
///
/// Types implementing [`Owner`] get a blanket [`CheckOwner`] impl that
/// compares against a single address (~20 CU).
///
/// ## Multi-owner (interface) accounts (T: CheckOwner)
///
/// ```ignore
/// // Validates owner == SPL Token OR Token-2022
/// pub token: &'info Account<InterfaceTokenAccount>,
/// ```
///
/// Types implementing [`CheckOwner`] directly use explicit comparison
/// chains instead of slice iteration, avoiding ~20-40 CU overhead.
///
/// ## Zero-copy access (T: ZeroCopyDeref)
///
/// When `T` implements [`ZeroCopyDeref`], `Account<T>` provides
/// `Deref`/`DerefMut` to the ZC companion struct:
///
/// ```ignore
/// let amount = ctx.accounts.token.amount(); // via Deref<Target = TokenAccountState>
/// ```
///
/// ## Borsh access (T: QuasarAccount)
///
/// When `T` implements [`QuasarAccount`], `Account<T>` provides
/// `.get()` / `.set()` for Borsh-style (de)serialization.
///
/// ## Polymorphic dispatch (T: InterfaceResolve)
///
/// When `T` implements [`InterfaceResolve`], `Account<T>` provides
/// `.resolve()` to dispatch to a program-specific resolved type:
///
/// ```ignore
/// match ctx.accounts.token.resolve()? {
///     TokenVariant::Spl(state) => { /* SPL Token specific */ }
///     TokenVariant::Token2022(state) => { /* Token-2022 specific */ }
/// }
/// ```
#[repr(transparent)]
pub struct Account<T> {
    view: AccountView,
    _marker: PhantomData<T>,
}

impl<T> AsAccountView for Account<T> {
    #[inline(always)]
    fn to_account_view(&self) -> &AccountView {
        &self.view
    }
}

impl<T> Account<T> {
    #[inline(always)]
    pub fn realloc(
        &self,
        new_space: usize,
        payer: &AccountView,
        rent: Option<&crate::sysvars::rent::Rent>,
    ) -> Result<(), ProgramError> {
        realloc_account(self.to_account_view(), new_space, payer, rent)
    }
}

impl<T: Owner> Account<T> {
    #[inline(always)]
    pub fn owner(&self) -> &'static Address {
        &T::OWNER
    }

    /// Close a program-owned account: drain lamports, reassign to system program,
    /// and resize to zero.
    ///
    /// Only works for accounts owned by the calling program (i.e. types
    /// implementing [`Owner`]). For token/mint accounts owned by the SPL Token
    /// or Token-2022 programs, use the CPI-based close via the token program.
    #[inline(always)]
    pub fn close(&self, destination: &AccountView) -> Result<(), ProgramError> {
        let view = self.to_account_view();
        destination.set_lamports(destination.lamports() + view.lamports());
        view.set_lamports(0);
        unsafe { view.assign(&SYSTEM_PROGRAM_ID) };
        view.resize(0)?;
        Ok(())
    }
}

impl<T: CheckOwner + AccountCheck> Account<T> {
    #[inline(always)]
    pub fn from_account_view(view: &AccountView) -> Result<&Self, ProgramError> {
        T::check_owner(view)?;
        T::check(view)?;
        Ok(unsafe { &*(view as *const AccountView as *const Self) })
    }

    /// # Safety (invalid_reference_casting)
    ///
    /// `Self` is `#[repr(transparent)]` over `AccountView`, which uses interior
    /// mutability through raw pointers to SVM account memory. The `&` → `&mut`
    /// cast does not create aliased mutable references to backing memory — all
    /// writes go through `AccountView`'s raw pointer methods. This pattern is
    /// standard in Solana frameworks (Pinocchio uses the same approach).
    #[inline(always)]
    #[allow(invalid_reference_casting, clippy::mut_from_ref)]
    pub fn from_account_view_mut(view: &AccountView) -> Result<&mut Self, ProgramError> {
        if !view.is_writable() {
            return Err(ProgramError::Immutable);
        }
        T::check_owner(view)?;
        T::check(view)?;
        Ok(unsafe { &mut *(view as *const AccountView as *mut Self) })
    }
}

impl<T: QuasarAccount> Account<T> {
    #[inline(always)]
    pub fn get(&self) -> Result<T, ProgramError> {
        let data = self.view.try_borrow()?;
        let disc = T::DISCRIMINATOR;
        if data.len() < disc.len() || &data[..disc.len()] != disc {
            return Err(ProgramError::InvalidAccountData);
        }
        T::deserialize(&data[disc.len()..])
    }

    #[inline(always)]
    pub fn set(&mut self, value: &T) -> Result<(), ProgramError> {
        let mut data = self.view.try_borrow_mut()?;
        let disc = T::DISCRIMINATOR;
        value.serialize(&mut data[disc.len()..])
    }
}

impl<T: ZeroCopyDeref> core::ops::Deref for Account<T> {
    type Target = T::Target;

    /// SAFETY: Bounds validated by `AccountCheck::check` during `from_account_view`.
    /// For fixed accounts, the target is a ZC companion struct with alignment 1.
    /// For dynamic accounts, the target is a `#[repr(transparent)]` View over AccountView.
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        T::deref_from(&self.view)
    }
}

impl<T: ZeroCopyDeref> core::ops::DerefMut for Account<T> {
    /// SAFETY: Same as Deref — bounds checked upstream.
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        T::deref_from_mut(&self.view)
    }
}

impl<T: InterfaceResolve> Account<T> {
    #[inline(always)]
    pub fn resolve(&self) -> Result<T::Resolved<'_>, ProgramError> {
        T::resolve(&self.view)
    }
}
