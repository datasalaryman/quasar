#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

#[derive(Accounts)]
pub struct BadGenericAccount<'info, T> {
    pub signer: &'info Signer,
    pub _marker: core::marker::PhantomData<T>,
}

fn main() {}
