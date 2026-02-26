use solana_address::Address;

/// Zero-copy layout for SPL Token mint accounts (82 bytes).
///
/// Fields use raw byte arrays for alignment-1 access. The layout is identical
/// for both SPL Token and Token-2022 (base mint data occupies the first 82 bytes).
#[repr(C)]
pub struct MintAccountState {
    mint_authority_flag: [u8; 4],
    mint_authority: Address,
    supply: [u8; 8],
    decimals: u8,
    is_initialized: u8,
    freeze_authority_flag: [u8; 4],
    freeze_authority: Address,
}

impl MintAccountState {
    pub const LEN: usize = core::mem::size_of::<MintAccountState>();

    #[inline(always)]
    pub fn has_mint_authority(&self) -> bool {
        self.mint_authority_flag[0] == 1
    }

    #[inline(always)]
    pub fn mint_authority(&self) -> Option<&Address> {
        if self.has_mint_authority() {
            Some(&self.mint_authority)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn mint_authority_unchecked(&self) -> &Address {
        &self.mint_authority
    }

    #[inline(always)]
    pub fn supply(&self) -> u64 {
        u64::from_le_bytes(self.supply)
    }

    #[inline(always)]
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    #[inline(always)]
    pub fn is_initialized(&self) -> bool {
        self.is_initialized != 0
    }

    #[inline(always)]
    pub fn has_freeze_authority(&self) -> bool {
        self.freeze_authority_flag[0] == 1
    }

    #[inline(always)]
    pub fn freeze_authority(&self) -> Option<&Address> {
        if self.has_freeze_authority() {
            Some(&self.freeze_authority)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn freeze_authority_unchecked(&self) -> &Address {
        &self.freeze_authority
    }
}

const _ASSERT_MINT_LEN: () = assert!(MintAccountState::LEN == 82);
const _ASSERT_MINT_ALIGN: () = assert!(core::mem::align_of::<MintAccountState>() == 1);
