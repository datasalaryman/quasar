use quasar_core::prelude::*;

use crate::constants::{SPL_TOKEN_ID, TOKEN_2022_ID};
use crate::cpi::TokenCpi;
use crate::state::{MintAccountState, TokenAccountState};

/// Token account type for the token interface — accepts accounts owned by
/// either SPL Token or Token-2022.
///
/// The base account layout (first 165 bytes) is identical for both programs.
/// Use with [`Account<InterfaceTokenAccount>`] in instruction structs:
///
/// ```ignore
/// pub from: &'info Account<InterfaceTokenAccount>,
/// ```
pub struct InterfaceTokenAccount;

impl AccountCheck for InterfaceTokenAccount {
    #[inline(always)]
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        if view.data_len() < TokenAccountState::LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        Ok(())
    }
}

impl CheckOwner for InterfaceTokenAccount {
    #[inline(always)]
    fn check_owner(view: &AccountView) -> Result<(), ProgramError> {
        if !view.owned_by(&SPL_TOKEN_ID) && !view.owned_by(&TOKEN_2022_ID) {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }
}

impl ZeroCopyDeref for InterfaceTokenAccount {
    type Target = TokenAccountState;

    #[inline(always)]
    fn deref_from(view: &AccountView) -> &Self::Target {
        unsafe { &*(view.data_ptr() as *const TokenAccountState) }
    }

    #[inline(always)]
    fn deref_from_mut(view: &AccountView) -> &mut Self::Target {
        unsafe { &mut *(view.data_ptr() as *mut TokenAccountState) }
    }
}

/// Mint account type for the token interface — accepts accounts owned by
/// either SPL Token or Token-2022.
///
/// The base mint layout (first 82 bytes) is identical for both programs.
/// Use with [`Account<InterfaceMintAccount>`] in instruction structs:
///
/// ```ignore
/// pub mint: &'info Account<InterfaceMintAccount>,
/// ```
pub struct InterfaceMintAccount;

impl AccountCheck for InterfaceMintAccount {
    #[inline(always)]
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        if view.data_len() < MintAccountState::LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        Ok(())
    }
}

impl CheckOwner for InterfaceMintAccount {
    #[inline(always)]
    fn check_owner(view: &AccountView) -> Result<(), ProgramError> {
        if !view.owned_by(&SPL_TOKEN_ID) && !view.owned_by(&TOKEN_2022_ID) {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }
}

impl ZeroCopyDeref for InterfaceMintAccount {
    type Target = MintAccountState;

    #[inline(always)]
    fn deref_from(view: &AccountView) -> &Self::Target {
        unsafe { &*(view.data_ptr() as *const MintAccountState) }
    }

    #[inline(always)]
    fn deref_from_mut(view: &AccountView) -> &mut Self::Target {
        unsafe { &mut *(view.data_ptr() as *mut MintAccountState) }
    }
}

/// Token interface program type — accepts either SPL Token or Token-2022.
///
/// Validates that the account is executable and its address matches one of
/// the two token program IDs. Provides the same CPI methods as [`TokenProgram`].
///
/// ```ignore
/// pub token_program: &'info TokenInterface,
/// ```
#[repr(transparent)]
pub struct TokenInterface {
    view: AccountView,
}

impl AsAccountView for TokenInterface {
    #[inline(always)]
    fn to_account_view(&self) -> &AccountView {
        &self.view
    }
}

impl TokenInterface {
    #[inline(always)]
    pub fn from_account_view(view: &AccountView) -> Result<&Self, ProgramError> {
        if !view.executable() {
            return Err(ProgramError::InvalidAccountData);
        }
        if view.address() != &SPL_TOKEN_ID && view.address() != &TOKEN_2022_ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &*(view as *const AccountView as *const Self) })
    }

    /// # Safety (invalid_reference_casting)
    ///
    /// `Self` is `#[repr(transparent)]` over `AccountView`, which uses
    /// interior mutability through raw pointers to SVM account memory.
    /// The SVM runtime manages lamports and data as separate mutable
    /// regions behind raw pointers — `AccountView` never holds Rust
    /// references to these regions. The `&` → `&mut` cast therefore
    /// does not create aliased mutable references; all writes go
    /// through `AccountView`'s raw pointer methods. This pattern is
    /// standard in Solana frameworks (Pinocchio uses the same approach).
    #[inline(always)]
    #[allow(invalid_reference_casting, clippy::mut_from_ref)]
    pub fn from_account_view_mut(view: &AccountView) -> Result<&mut Self, ProgramError> {
        if !view.is_writable() {
            return Err(ProgramError::Immutable);
        }
        if !view.executable() {
            return Err(ProgramError::InvalidAccountData);
        }
        if view.address() != &SPL_TOKEN_ID && view.address() != &TOKEN_2022_ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &mut *(view as *const AccountView as *mut Self) })
    }
}

impl TokenCpi for TokenInterface {}
