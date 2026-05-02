//! Context structs for capability trait methods.
//!
//! These provide the public input surface for capability traits. The derive
//! constructs them directly when emitting validation and init-contributor
//! calls.

use quasar_lang::prelude::AccountView;

/// Context for token account validation.
///
/// `token_program` is optional: for concrete `Account<Token>` types,
/// `AccountLoad::check` already validated the owner. Only
/// `InterfaceAccount<Token>` needs the runtime program check.
pub struct TokenCheckCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: Option<&'a AccountView>,
}

/// Context for mint account validation.
///
/// `token_program` is optional: concrete types already have owner validated.
/// `decimals` is optional: defaults to "don't check" when None.
pub struct MintCheckCtx<'a> {
    pub decimals: Option<u8>,
    pub authority: &'a AccountView,
    pub freeze_authority: Option<&'a AccountView>,
    pub token_program: Option<&'a AccountView>,
}

/// Context for associated token address + token data validation.
///
/// `token_program` is optional: for concrete `Account<Token>`, the program
/// is known from the owner. When None, uses the account's on-chain owner
/// for ATA address derivation (safe — AccountLoad validated the owner).
pub struct AssociatedTokenCheckCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: Option<&'a AccountView>,
}
