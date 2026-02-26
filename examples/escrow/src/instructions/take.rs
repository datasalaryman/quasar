use quasar_core::prelude::*;
use quasar_spl::{InitToken, MintAccount, TokenAccount, TokenClose, TokenCpi, TokenProgram};

use crate::events::TakeEvent;
use crate::state::EscrowAccount;

#[derive(Accounts)]
pub struct Take<'info> {
    pub taker: &'info mut Signer,
    #[account(
        has_one = maker,
        has_one = maker_ta_b,
        constraint = escrow.receive > 0,
        seeds = [b"escrow", maker],
        bump = escrow.bump
    )]
    pub escrow: &'info mut Account<EscrowAccount>,
    pub maker: &'info mut UncheckedAccount,
    pub mint_a: &'info Account<MintAccount>,
    pub mint_b: &'info Account<MintAccount>,
    pub taker_ta_a: &'info mut Initialize<TokenAccount>,
    pub taker_ta_b: &'info mut Account<TokenAccount>,
    pub maker_ta_b: &'info mut Initialize<TokenAccount>,
    pub vault_ta_a: &'info mut Account<TokenAccount>,
    pub rent: &'info Sysvar<Rent>,
    pub token_program: &'info TokenProgram,
    pub system_program: &'info SystemProgram,
}

impl<'info> Take<'info> {
    #[inline(always)]
    pub fn init_accounts(&self) -> Result<(), ProgramError> {
        let rent = Some(&**self.rent);

        self.taker_ta_a.init_if_needed(
            self.system_program,
            self.taker,
            self.token_program,
            self.mint_a,
            self.taker.address(),
            rent,
        )?;

        self.maker_ta_b.init_if_needed(
            self.system_program,
            self.taker,
            self.token_program,
            self.mint_b,
            self.maker.address(),
            rent,
        )
    }

    #[inline(always)]
    pub fn transfer_tokens(&mut self) -> Result<(), ProgramError> {
        self.token_program
            .transfer(
                self.taker_ta_b,
                self.maker_ta_b,
                self.taker,
                self.escrow.receive,
            )
            .invoke()
    }

    #[inline(always)]
    pub fn withdraw_tokens_and_close(&mut self, bumps: &TakeBumps) -> Result<(), ProgramError> {
        let seeds = bumps.escrow_seeds();

        self.token_program
            .transfer(
                self.vault_ta_a,
                self.taker_ta_a,
                self.escrow,
                self.vault_ta_a.amount(),
            )
            .invoke_signed(&seeds)?;

        self.vault_ta_a
            .close(self.token_program, self.taker, self.escrow)
            .invoke_signed(&seeds)
    }

    #[inline(always)]
    pub fn emit_event(&self) -> Result<(), ProgramError> {
        emit!(TakeEvent {
            escrow: *self.escrow.address(),
        });
        Ok(())
    }

    #[inline(always)]
    pub fn close_escrow(&mut self) -> Result<(), ProgramError> {
        self.escrow.close(self.maker.to_account_view())
    }
}
