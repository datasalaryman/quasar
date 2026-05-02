#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{accounts::token, TokenProgram, *};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct TypoField {
    pub authority: Signer,
    pub mint: Account<Mint>,

    #[account(token(mint = mnit, authority = authority, token_program = token_program))]
    pub vault: Account<Token>,

    pub token_program: Program<TokenProgram>,
}

fn main() {}
