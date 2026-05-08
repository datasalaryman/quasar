use {super::deposit::VaultPda, quasar_lang::prelude::*};

#[derive(Accounts)]
pub struct Withdraw {
    #[account(mut)]
    pub user: Signer,
    #[account(mut, address = VaultPda::seeds(user.address()))]
    pub vault: UncheckedAccount,
}

impl Withdraw {
    #[inline(always)]
    pub fn withdraw(&self, amount: u64) -> Result<(), ProgramError> {
        let vault = self.vault.to_account_view();
        let user = self.user.to_account_view();
        let vault_lamports = vault
            .lamports()
            .checked_sub(amount)
            .ok_or(ProgramError::InsufficientFunds)?;
        let user_lamports = user
            .lamports()
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        set_lamports(vault, vault_lamports);
        set_lamports(user, user_lamports);
        Ok(())
    }
}
