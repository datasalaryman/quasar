use quasar_core::prelude::*;

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub user: &'info mut Signer,
    #[account(mut, seeds = [b"vault", user], bump)]
    pub vault: &'info mut UncheckedAccount,
}

impl<'info> Withdraw<'info> {
    #[inline(always)]
    pub fn withdraw(&self, amount: u64) -> Result<(), ProgramError> {
        let vault = self.vault.to_account_view();
        let user = self.user.to_account_view();
        vault.set_lamports(vault.lamports() - amount);
        user.set_lamports(user.lamports() + amount);
        Ok(())
    }
}
