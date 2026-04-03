#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct BadDupAttr<'info> {
    pub payer: &'info Signer,
    /// CHECK: test-only duplicate alias.
    #[account(mut, dup)]
    pub authority: &'info UncheckedAccount,
}

fn main() {}
