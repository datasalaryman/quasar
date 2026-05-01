use {solana_account_view::AccountView, solana_program_error::ProgramError};

/// Validates discriminator bytes at offset `0..disc_len`.
///
/// Requires `Self: crate::traits::Discriminator` to provide
/// `DISCRIMINATOR: &'static [u8]`.
pub trait Discriminator: crate::traits::Discriminator {
    #[inline(always)]
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        let data = unsafe { view.borrow_unchecked() };
        let disc = Self::DISCRIMINATOR;
        if data.len() < disc.len() {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let mut i = 0;
        while i < disc.len() {
            if data[i] != disc[i] {
                return Err(ProgramError::InvalidAccountData);
            }
            i += 1;
        }
        Ok(())
    }
}
