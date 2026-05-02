#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::accounts::mint;
use quasar_spl::{Mint, TokenProgram};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct BadFreeze {
    pub authority: Signer,

    #[account(mut, mint(authority = authority, freeze_authority = nonexistent))]
    pub mint: Account<Mint>,

    pub token_program: Program<TokenProgram>,
}

fn main() {}
