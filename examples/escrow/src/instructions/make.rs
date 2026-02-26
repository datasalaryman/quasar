use quasar_core::prelude::*;
use quasar_spl::{InitToken, MintAccount, TokenAccount, TokenCpi, TokenProgram};

use crate::{events::MakeEvent, state::EscrowAccount};

#[derive(Accounts)]
pub struct Make<'info> {
    pub maker: &'info mut Signer,
    #[account(seeds = [b"escrow", maker], bump)]
    pub escrow: &'info mut Initialize<EscrowAccount>,
    pub mint_a: &'info Account<MintAccount>,
    pub mint_b: &'info Account<MintAccount>,
    pub maker_ta_a: &'info mut Account<TokenAccount>,
    pub maker_ta_b: &'info mut Initialize<TokenAccount>,
    pub vault_ta_a: &'info mut Initialize<TokenAccount>,
    pub rent: &'info Sysvar<Rent>,
    pub token_program: &'info TokenProgram,
    pub system_program: &'info SystemProgram,
}

impl<'info> Make<'info> {
    #[inline(always)]
    pub fn init_accounts(&self) -> Result<(), ProgramError> {
        let rent = Some(&**self.rent);

        self.vault_ta_a.init_if_needed(
            self.system_program,
            self.maker,
            self.token_program,
            self.mint_a,
            self.escrow.address(),
            rent,
        )?;

        self.maker_ta_b.init_if_needed(
            self.system_program,
            self.maker,
            self.token_program,
            self.mint_b,
            self.maker.address(),
            rent,
        )
    }

    #[inline(always)]
    pub fn make_escrow(&mut self, receive: u64, bumps: &MakeBumps) -> Result<(), ProgramError> {
        let seeds = bumps.escrow_seeds();

        EscrowAccount {
            maker: *self.maker.address(),
            mint_a: *self.mint_a.address(),
            mint_b: *self.mint_b.address(),
            maker_ta_b: *self.maker_ta_b.address(),
            receive,
            bump: bumps.escrow,
        }
        .init_signed(
            self.escrow,
            self.maker.to_account_view(),
            Some(&**self.rent),
            &[quasar_core::cpi::Signer::from(&seeds)],
        )
    }

    #[inline(always)]
    pub fn emit_event(&self, deposit: u64, receive: u64) -> Result<(), ProgramError> {
        emit!(MakeEvent {
            escrow: *self.escrow.address(),
            maker: *self.maker.address(),
            mint_a: *self.mint_a.address(),
            mint_b: *self.mint_b.address(),
            deposit,
            receive,
        });
        Ok(())
    }

    #[inline(always)]
    pub fn deposit_tokens(&mut self, amount: u64) -> Result<(), ProgramError> {
        self.token_program
            .transfer(self.maker_ta_a, self.vault_ta_a, self.maker, amount)
            .invoke()
    }
}
