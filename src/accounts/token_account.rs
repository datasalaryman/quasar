use crate::prelude::*;

pub struct TokenAccount;

impl AccountCheck for TokenAccount {}

impl Owner for TokenAccount {
    const OWNER: Address = Address::new_from_array([
        6, 221, 246, 225, 215, 101, 161, 147,
        217, 203, 225, 70, 206, 235, 121, 172,
        28, 180, 133, 237, 95, 91, 55, 145,
        58, 140, 245, 133, 126, 255, 0, 169,
    ]);
}

impl core::ops::Deref for Account<TokenAccount> {
    type Target = crate::token::TokenAccountState;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.to_account_view().borrow_unchecked().as_ptr() as *const Self::Target) }
    }
}
