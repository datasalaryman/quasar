#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 1)]
pub struct Data {
    pub value: PodU64,
}

// ERROR: `realloc = ...` cannot be used with `init`
#[derive(Accounts)]
pub struct Bad {
    #[account(mut)]
    pub payer: Signer,
    pub system_program: Program<SystemProgram>,

    #[account(init, payer = payer, realloc = 100)]
    pub target: Account<Data>,
}

fn main() {}
