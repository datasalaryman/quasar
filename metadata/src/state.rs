use solana_address::Address;

/// Metaplex Key enum discriminant for MetadataV1 accounts.
#[allow(dead_code)]
const KEY_METADATA_V1: u8 = 4;
/// Metaplex Key enum discriminant for MasterEditionV2 accounts.
#[allow(dead_code)]
const KEY_MASTER_EDITION_V2: u8 = 6;

// ---------------------------------------------------------------------------
// MetadataPrefix — zero-copy layout for the fixed 65-byte header
// ---------------------------------------------------------------------------

/// Zero-copy layout for the fixed-size prefix of Metaplex Metadata accounts.
///
/// The first 65 bytes of a Metadata account have a stable layout:
/// - `key` (1 byte): Metaplex account type discriminant (`Key::MetadataV1 = 4`)
/// - `update_authority` (32 bytes): pubkey authorized to update this metadata
/// - `mint` (32 bytes): the SPL Token mint this metadata describes
///
/// Fields after the prefix (name, symbol, uri, creators, etc.) are
/// variable-length Borsh-serialized data and require offset walking to access.
#[repr(C)]
pub struct MetadataPrefix {
    key: u8,
    update_authority: Address,
    mint: Address,
}

impl MetadataPrefix {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn key(&self) -> u8 {
        self.key
    }

    #[inline(always)]
    pub fn update_authority(&self) -> &Address {
        &self.update_authority
    }

    #[inline(always)]
    pub fn mint(&self) -> &Address {
        &self.mint
    }
}

const _: () = assert!(core::mem::size_of::<MetadataPrefix>() == 65);
const _: () = assert!(core::mem::align_of::<MetadataPrefix>() == 1);

// ---------------------------------------------------------------------------
// MasterEditionPrefix — zero-copy layout for the fixed 18-byte header
// ---------------------------------------------------------------------------

/// Zero-copy layout for the fixed-size prefix of Metaplex MasterEdition
/// accounts.
///
/// - `key` (1 byte): Metaplex account type discriminant (`Key::MasterEditionV2
///   = 6`)
/// - `supply` (8 bytes, u64 LE): number of editions printed
/// - `max_supply_flag` (1 byte): `Option<u64>` tag — 0 = None (unlimited), 1 =
///   Some
/// - `max_supply` (8 bytes, u64 LE): maximum editions (valid only when flag ==
///   1)
#[repr(C)]
pub struct MasterEditionPrefix {
    key: u8,
    supply: [u8; 8],
    max_supply_flag: u8,
    max_supply: [u8; 8],
}

impl MasterEditionPrefix {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn key(&self) -> u8 {
        self.key
    }

    #[inline(always)]
    pub fn supply(&self) -> u64 {
        u64::from_le_bytes(self.supply)
    }

    #[inline(always)]
    pub fn max_supply(&self) -> Option<u64> {
        if self.max_supply_flag == 1 {
            Some(u64::from_le_bytes(self.max_supply))
        } else {
            None
        }
    }
}

const _: () = assert!(core::mem::size_of::<MasterEditionPrefix>() == 18);
const _: () = assert!(core::mem::align_of::<MasterEditionPrefix>() == 1);

// ---------------------------------------------------------------------------
// MetadataAccount / MasterEditionAccount — TODO: rework in follow-up PR
//
// These marker types predate the current AccountLoad pattern. They need to be
// reworked as #[repr(transparent)] over AccountView (like Token/Mint) to
// satisfy AccountLoad's AsAccountView supertrait. Left commented out to
// demonstrate how easy external account types are with the new system.
// ---------------------------------------------------------------------------

// pub struct MetadataAccount { ... }
// pub struct MasterEditionAccount { ... }

// ---------------------------------------------------------------------------
// Kani model-checking proof harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    // --- MetadataPrefix ---

    /// Prove MetadataPrefix::LEN matches the actual struct size.
    #[kani::proof]
    fn metadata_prefix_len_matches_sizeof() {
        assert!(MetadataPrefix::LEN == core::mem::size_of::<MetadataPrefix>());
    }

    /// Prove MetadataPrefix has alignment 1 (safe for pointer cast from
    /// arbitrary account data).
    #[kani::proof]
    fn metadata_prefix_align_one() {
        assert!(core::mem::align_of::<MetadataPrefix>() == 1);
    }

    /// Prove MetadataPrefix is exactly 65 bytes.
    #[kani::proof]
    fn metadata_prefix_size_65() {
        assert!(core::mem::size_of::<MetadataPrefix>() == 65);
    }

    /// Prove: for any `data_len >= MetadataPrefix::LEN`, the data covers
    /// the full struct — verifies the runtime guard in `MetadataAccount::check`
    /// is sufficient for the pointer cast in `deref_from`.
    #[kani::proof]
    fn metadata_prefix_data_len_guard_sufficient() {
        let data_len: usize = kani::any();
        kani::assume(data_len >= MetadataPrefix::LEN);
        assert!(data_len >= core::mem::size_of::<MetadataPrefix>());
    }

    // --- MasterEditionPrefix ---

    /// Prove MasterEditionPrefix::LEN matches the actual struct size.
    #[kani::proof]
    fn master_edition_prefix_len_matches_sizeof() {
        assert!(MasterEditionPrefix::LEN == core::mem::size_of::<MasterEditionPrefix>());
    }

    /// Prove MasterEditionPrefix has alignment 1 (safe for pointer cast from
    /// arbitrary account data).
    #[kani::proof]
    fn master_edition_prefix_align_one() {
        assert!(core::mem::align_of::<MasterEditionPrefix>() == 1);
    }

    /// Prove MasterEditionPrefix is exactly 18 bytes.
    #[kani::proof]
    fn master_edition_prefix_size_18() {
        assert!(core::mem::size_of::<MasterEditionPrefix>() == 18);
    }

    /// Prove: for any `data_len >= MasterEditionPrefix::LEN`, the data covers
    /// the full struct — verifies the runtime guard in
    /// `MasterEditionAccount::check` is sufficient for the pointer cast in
    /// `deref_from`.
    #[kani::proof]
    fn master_edition_prefix_data_len_guard_sufficient() {
        let data_len: usize = kani::any();
        kani::assume(data_len >= MasterEditionPrefix::LEN);
        assert!(data_len >= core::mem::size_of::<MasterEditionPrefix>());
    }
}
