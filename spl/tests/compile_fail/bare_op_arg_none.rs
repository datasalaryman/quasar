#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::accounts::{mint, token};
use quasar_spl::{Mint, Token, TokenProgram};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct BadNone {
    pub authority: Signer,
    pub mint: Account<Mint>,

    #[account(token(mint = None, authority = authority))]
    pub vault: Account<Token>,

    pub token_program: Program<TokenProgram>,
}

fn main() {}
