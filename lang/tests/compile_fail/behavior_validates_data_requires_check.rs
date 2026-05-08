#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 42)]
pub struct MyData {
    pub value: u64,
}

mod fast_guard {
    use quasar_lang::prelude::*;

    pub struct Args;

    pub struct ArgsBuilder;

    impl Args {
        pub fn builder() -> ArgsBuilder {
            ArgsBuilder
        }
    }

    impl ArgsBuilder {
        pub fn build_check(self) -> Result<Args, ProgramError> {
            Ok(Args)
        }

        pub fn build_init(self) -> Result<Args, ProgramError> {
            Ok(Args)
        }

        pub fn build_exit(self) -> Result<Args, ProgramError> {
            Ok(Args)
        }
    }

    pub struct Behavior;

    impl AccountBehavior<Account<super::MyData>> for Behavior {
        type Args<'a> = Args;

        const RUN_CHECK: bool = false;
        const VALIDATES_ACCOUNT_DATA: bool = true;
    }
}

#[derive(Accounts)]
pub struct Bad {
    #[account(fast_guard())]
    pub data: Account<MyData>,
}

fn main() {}
