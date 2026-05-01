use {crate::account_layout::AccountLayout, solana_account_view::AccountView, solana_program_error::ProgramError};

/// Validates that account data is at least `DATA_OFFSET + DATA_SIZE` bytes.
pub trait DataLen: AccountLayout {
    #[inline(always)]
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        if view.data_len() < Self::DATA_OFFSET + Self::DATA_SIZE {
            return Err(ProgramError::AccountDataTooSmall);
        }
        Ok(())
    }
}
