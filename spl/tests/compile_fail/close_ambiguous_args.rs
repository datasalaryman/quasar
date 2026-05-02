#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{accounts::token_close, TokenProgram};

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 7)]
pub struct Data {
    pub value: u64,
}

#[derive(Accounts)]
pub struct BadCloseArgs {
    #[account(mut)]
    pub dest: Signer,

    #[account(mut, token_close(dest = dest, authority = dest, token_program = token_program))]
    pub data: Account<Data>,

    pub token_program: Program<TokenProgram>,
}

fn main() {}
