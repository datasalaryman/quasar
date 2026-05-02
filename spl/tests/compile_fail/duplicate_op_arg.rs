#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{TokenProgram, *};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct DuplicateArgs {
    pub authority: Signer,
    pub mint: Account<Mint>,

    #[account(token(mint = mint, authority = authority, authority = mint))]
    pub vault: Account<Token>,

    pub token_program: Program<TokenProgram>,
}

fn main() {}
