#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_spl::{
    AssociatedTokenProgram, Token, TokenProgram,
};

solana_address::declare_id!("11111111111111111111111111111112");

// ERROR: only one init contributor group allowed per field
#[derive(Accounts)]
pub struct Bad {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init, payer = payer,
        token(mint = mint, authority = payer),
        associated_token(mint = mint, authority = payer),
    )]
    pub vault: Account<Token>,
    pub mint: Account<quasar_spl::Mint>,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
    pub ata_program: Program<AssociatedTokenProgram>,
}

fn main() {}
