//! SPL Token program integration for Quasar.
//!
//! Provides zero-copy account types and CPI methods for the SPL Token program
//! and Token-2022 (Token Extensions) program.
//!
//! # Account types
//!
//! | Type | Owner check | Deref target | Use when |
//! |------|-------------|--------------|----------|
//! | `Account<Token>` | SPL Token only | [`TokenAccountState`] | Token accounts (incl. ATAs) for SPL Token |
//! | `Account<Mint>` | SPL Token only | [`MintAccountState`] | Mint owned by Token |
//! | `InterfaceAccount<Token>` | SPL Token **or** Token-2022 | [`TokenAccountState`] | Token accounts (incl. ATAs) for either program |
//! | `InterfaceAccount<Mint>` | SPL Token **or** Token-2022 | [`MintAccountState`] | Mint from either program |
//!
//! # Program types
//!
//! | Type | Accepts | Use when |
//! |------|---------|----------|
//! | `Program<TokenProgram>` | SPL Token only | CPI to Token program |
//! | [`TokenInterface`] | SPL Token **or** Token-2022 | CPI to either program |
//!
//! # CPI methods
//!
//! Both `Program<TokenProgram>` and [`TokenInterface`] expose the same CPI
//! methods. All methods return a `CpiCall` that can be invoked with `.invoke()`
//! or `.invoke_signed()`:
//!
//! ```ignore
//! ctx.accounts.token_program
//!     .transfer(&from, &to, &authority, amount)
//!     .invoke();
//! ```
//!
//! # Token lifecycle
//!
//! Use `#[account(init)]` to auto-create token accounts, mints, and ATAs.
//! The derive macro handles `create_account` + `initialize_*` CPI calls.
//!
//! For closing, use `close_account` on the token program directly:
//!
//! ```ignore
//! self.token_program.close_account(&self.vault, &self.maker, &self.escrow)
//!     .invoke_signed(&seeds);
//! ```

#![no_std]

/// Implements `AccountCheck`, `TokenClose`, and `TokenSweep` for a token
/// account type (Token / Token2022). All token account types share the same
/// validation logic and close/sweep dispatch.
macro_rules! impl_token_account_traits {
    ($ty:ty) => {
        impl AccountCheck for $ty {
            #[inline(always)]
            fn check(view: &AccountView) -> Result<(), ProgramError> {
                if quasar_lang::utils::hint::unlikely(
                    view.data_len() < crate::state::TokenAccountState::LEN,
                ) {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                Ok(())
            }
        }

        impl crate::ops::close::TokenClose for $ty {
            #[inline(always)]
            fn close(
                view: &mut AccountView,
                dest: &AccountView,
                authority: &AccountView,
                token_program: &AccountView,
            ) -> Result<(), ProgramError> {
                crate::exit::close_token_account(
                    token_program,
                    unsafe { &*(view as *const AccountView) },
                    dest,
                    authority,
                )
            }
        }

        impl crate::ops::sweep::TokenSweep for $ty {
            #[inline(always)]
            fn sweep(
                view: &AccountView,
                receiver: &AccountView,
                mint: &AccountView,
                authority: &AccountView,
                token_program: &AccountView,
            ) -> Result<(), ProgramError> {
                crate::exit::sweep_token_account(token_program, view, mint, receiver, authority)
            }
        }
    };
}

/// Implements `AccountCheck` for a mint account type (Mint / Mint2022).
macro_rules! impl_mint_account_check {
    ($ty:ty) => {
        impl AccountCheck for $ty {
            #[inline(always)]
            fn check(view: &AccountView) -> Result<(), ProgramError> {
                if quasar_lang::utils::hint::unlikely(
                    view.data_len() < crate::state::MintAccountState::LEN,
                ) {
                    return Err(ProgramError::AccountDataTooSmall);
                }
                Ok(())
            }
        }
    };
}

/// Implements `AccountInit` for a token account type (Token / Token2022).
/// Both dispatch to the same init_token_account / init_ata helpers.
macro_rules! impl_token_account_init {
    ($ty:ty) => {
        impl quasar_lang::account_init::AccountInit for $ty {
            type InitParams<'a> = crate::token::TokenInitParams<'a>;

            #[inline(always)]
            fn init<'a>(
                ctx: quasar_lang::account_init::InitCtx<'a>,
                params: &Self::InitParams<'a>,
            ) -> Result<(), ProgramError> {
                match &params.kind {
                    Some(crate::token::TokenInitKind::Token {
                        mint,
                        authority,
                        token_program,
                    }) => crate::init::init_token_account(
                        ctx.payer,
                        ctx.target,
                        token_program,
                        mint,
                        authority,
                        ctx.signers,
                        ctx.rent,
                    ),
                    Some(crate::token::TokenInitKind::AssociatedToken {
                        mint,
                        authority,
                        token_program,
                        system_program,
                        ata_program,
                        idempotent,
                    }) => {
                        crate::validate_ata_program_id(ata_program)?;
                        crate::validate_token_program_id(token_program)?;
                        crate::validate_system_program_id(system_program)?;
                        crate::init::init_ata(
                            ata_program,
                            ctx.payer,
                            ctx.target,
                            authority,
                            mint,
                            system_program,
                            token_program,
                            *idempotent,
                        )
                    }
                    None => Err(ProgramError::InvalidAccountData),
                }
            }
        }
    };
}

/// Implements `AccountInit` for a mint account type (Mint / Mint2022).
/// Both dispatch to the same init_mint_account helper.
macro_rules! impl_mint_account_init {
    ($ty:ty) => {
        impl quasar_lang::account_init::AccountInit for $ty {
            type InitParams<'a> = crate::token::MintInitParams<'a>;

            #[inline(always)]
            fn init<'a>(
                ctx: quasar_lang::account_init::InitCtx<'a>,
                params: &Self::InitParams<'a>,
            ) -> Result<(), ProgramError> {
                let decimals = params.decimals.ok_or(ProgramError::InvalidAccountData)?;
                let authority = params.authority.ok_or(ProgramError::InvalidAccountData)?;
                let token_program = params
                    .token_program
                    .ok_or(ProgramError::InvalidAccountData)?;
                crate::init::init_mint_account(
                    ctx.payer,
                    ctx.target,
                    token_program,
                    decimals,
                    authority,
                    params.freeze_authority,
                    ctx.signers,
                    ctx.rent,
                )
            }
        }
    };
}

mod associated_token;
mod constants;
mod exit;
mod init;
mod instructions;
mod interface;
/// Op-dispatch implementations for SPL token operations.
pub mod ops;
mod state;
mod token;
mod token_2022;
mod validate;

pub use {
    associated_token::{
        create as ata_create, create_idempotent as ata_create_idempotent,
        get_associated_token_address_const, get_associated_token_address_with_program_const,
        AssociatedTokenCpi, AssociatedTokenProgram,
    },
    constants::{ATA_PROGRAM_ID, SPL_TOKEN_ID, TOKEN_2022_ID},
    exit::{close_token_account, sweep_token_account},
    init::{init_ata, init_mint_account, init_token_account},
    instructions::{initialize_account3, initialize_mint2, TokenCpi},
    interface::TokenInterface,
    quasar_lang::prelude::InterfaceAccount,
    state::{COption, MintAccountState, TokenAccountState},
    token::{Mint, Token, TokenProgram},
    token_2022::{Mint2022, Token2022, Token2022Program},
    validate::{
        validate_ata, validate_ata_program_id, validate_mint, validate_system_program_id,
        validate_token_account, validate_token_program_id,
    },
};
