#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: token_close requires mut but field is not mut
#[derive(Accounts)]
pub struct Bad {
    pub receiver: Signer,
    pub token_program: Program<TokenProgram>,

    #[account(token_close(dest = receiver, authority = receiver, token_program = token_program))]
    pub vault: Account<Token>,
}

fn main() {}
