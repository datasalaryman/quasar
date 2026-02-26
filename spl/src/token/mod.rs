use quasar_core::prelude::*;

use crate::constants::SPL_TOKEN_ID;
use crate::cpi::TokenCpi;
use crate::state::{MintAccountState, TokenAccountState};

quasar_core::define_account!(pub struct TokenProgram => [checks::Executable, checks::Address]);

impl Program for TokenProgram {
    const ID: Address = SPL_TOKEN_ID;
}

/// Token account owned by the SPL Token program.
pub struct TokenAccount;
impl_single_owner!(TokenAccount, SPL_TOKEN_ID, TokenAccountState);

/// Mint account owned by the SPL Token program.
pub struct MintAccount;
impl_single_owner!(MintAccount, SPL_TOKEN_ID, MintAccountState);

impl TokenCpi for TokenProgram {}
