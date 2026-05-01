//! PDA check — address verification is now done via `AddressVerify` in the
//! derive. This module is retained for backward compatibility.

/// PDA address check struct. Retained for backward compatibility.
/// The derive now uses `AddressVerify` directly instead.
pub struct Check<'a> {
    pub expected: &'a solana_address::Address,
    pub bump_out: &'a mut u8,
}
