#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::*;

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: `sweep(...)` must appear before `close(...)` — wrong ordering is always a bug
#[derive(Accounts)]
pub struct Bad {
    #[account(mut)]
    pub receiver: Signer,
    pub mint: Account<Mint>,
    pub token_program: Program<TokenProgram>,

    #[account(mut,
        close(dest = receiver, authority = receiver),
        sweep(receiver = receiver, mint = mint, authority = receiver)
    )]
    pub vault: Account<Token>,
}

fn main() {}
