#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

#[derive(Accounts)]
pub struct BadAllow {
    #[account(allow(unconstrained))]
    pub account: UncheckedAccount,
}

fn main() {}
