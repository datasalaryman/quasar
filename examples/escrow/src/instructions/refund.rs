use quasar_core::prelude::*;
use quasar_spl::{InitToken, MintAccount, TokenAccount, TokenClose, TokenCpi, TokenProgram};

use crate::{events::RefundEvent, state::EscrowAccount};

#[derive(Accounts)]
pub struct Refund<'info> {
    pub maker: &'info mut Signer,
    #[account(
        has_one = maker,
        seeds = [b"escrow", maker],
        bump = escrow.bump
    )]
    pub escrow: &'info mut Account<EscrowAccount>,
    pub mint_a: &'info Account<MintAccount>,
    pub maker_ta_a: &'info mut Initialize<TokenAccount>,
    pub vault_ta_a: &'info mut Account<TokenAccount>,
    pub rent: &'info Sysvar<Rent>,
    pub token_program: &'info TokenProgram,
    pub system_program: &'info SystemProgram,
}

impl<'info> Refund<'info> {
    #[inline(always)]
    pub fn init_accounts(&self) -> Result<(), ProgramError> {
        self.maker_ta_a.init_if_needed(
            self.system_program,
            self.maker,
            self.token_program,
            self.mint_a,
            self.maker.address(),
            Some(&**self.rent),
        )
    }

    #[inline(always)]
    pub fn withdraw_tokens_and_close(&mut self, bumps: &RefundBumps) -> Result<(), ProgramError> {
        let seeds = bumps.escrow_seeds();

        self.token_program
            .transfer(
                self.vault_ta_a,
                self.maker_ta_a,
                self.escrow,
                self.vault_ta_a.amount(),
            )
            .invoke_signed(&seeds)?;

        self.vault_ta_a
            .close(self.token_program, self.maker, self.escrow)
            .invoke_signed(&seeds)
    }

    #[inline(always)]
    pub fn emit_event(&self) -> Result<(), ProgramError> {
        emit!(RefundEvent {
            escrow: *self.escrow.address(),
        });
        Ok(())
    }

    #[inline(always)]
    pub fn close_escrow(&mut self) -> Result<(), ProgramError> {
        self.escrow.close(self.maker.to_account_view())
    }
}
