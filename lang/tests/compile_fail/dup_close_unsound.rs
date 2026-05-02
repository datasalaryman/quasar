#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::*;

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: `dup` cannot be used with `close` — mutation on aliased accounts is unsound
#[derive(Accounts)]
pub struct Bad {
    #[account(mut)]
    pub payer: Signer,
    pub token_program: Program<TokenProgram>,

    /// CHECK: testing dup + close
    #[account(mut, dup, close(dest = payer, authority = payer))]
    pub vault: Account<Token>,
}

fn main() {}
