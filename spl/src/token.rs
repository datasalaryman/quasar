use {
    crate::{
        constants::{SPL_TOKEN_BYTES, SPL_TOKEN_ID, TOKEN_2022_ID},
        instructions::TokenCpi,
        state::{MintAccountState, TokenAccountState},
    },
    quasar_lang::{prelude::*, traits::Id},
};

quasar_lang::define_account!(
    /// Token account data — validates owner is SPL Token program.
    ///
    /// Use as `Account<Token>` for single-program token accounts,
    /// or `InterfaceAccount<Token>` to accept both SPL Token and Token-2022.
    pub struct Token => [checks::Owner]: TokenAccountState
);

impl Owner for Token {
    const OWNER: Address = SPL_TOKEN_ID;
}

// SPL Token program marker. Use as `Program<TokenProgram>`.
quasar_lang::define_account!(pub struct TokenProgram => [checks::Executable, checks::Address]);

impl Id for TokenProgram {
    const ID: Address = Address::new_from_array(SPL_TOKEN_BYTES);
}

quasar_lang::define_account!(
    /// Mint account — validates owner is SPL Token program.
    ///
    /// Use as `Account<Mint>` for single-program mints,
    /// or `InterfaceAccount<Mint>` to accept both SPL Token and Token-2022.
    pub struct Mint => [checks::Owner]: MintAccountState
);

impl Owner for Mint {
    const OWNER: Address = SPL_TOKEN_ID;
}

/// Valid owner programs for `InterfaceAccount<Token>` and
/// `InterfaceAccount<Mint>` — accepts accounts owned by either SPL Token
/// or Token-2022.
///
/// Note: `Account<Token>` does NOT use this trait. It validates via
/// `checks::Owner` which checks only `SPL_TOKEN_ID`. The `Owners` trait
/// is consumed exclusively by `InterfaceAccount<T>` for multi-program
/// acceptance.
static SPL_TOKEN_OWNERS: [Address; 2] = [SPL_TOKEN_ID, TOKEN_2022_ID];

impl quasar_lang::traits::Owners for Token {
    #[inline(always)]
    fn owners() -> &'static [Address] {
        &SPL_TOKEN_OWNERS
    }
}

impl quasar_lang::traits::Owners for Mint {
    #[inline(always)]
    fn owners() -> &'static [Address] {
        &SPL_TOKEN_OWNERS
    }
}

impl TokenCpi for Program<TokenProgram> {}

// ---------------------------------------------------------------------------
// Shared trait impls (AccountCheck, TokenClose, TokenSweep, AccountInit)
// ---------------------------------------------------------------------------

impl_token_account_traits!(Token);
impl_mint_account_check!(Mint);
impl_token_account_init!(Token);
impl_mint_account_init!(Mint);

// ---------------------------------------------------------------------------
// Init param types (shared by Token2022/Mint2022)
// ---------------------------------------------------------------------------

/// Init kind: direct token account or ATA.
pub enum TokenInitKind<'a> {
    /// Direct token account init via system program + initialize_account3.
    Token {
        mint: &'a AccountView,
        authority: &'a Address,
        token_program: &'a AccountView,
    },
    /// ATA init via the associated token program.
    AssociatedToken {
        mint: &'a AccountView,
        authority: &'a AccountView,
        token_program: &'a AccountView,
        system_program: &'a AccountView,
        ata_program: &'a AccountView,
        idempotent: bool,
    },
}

/// Init params for token account creation via CPI.
#[derive(Default)]
pub struct TokenInitParams<'a> {
    pub kind: Option<TokenInitKind<'a>>,
}

/// Init params for mint account creation via CPI.
#[derive(Default)]
pub struct MintInitParams<'a> {
    pub decimals: Option<u8>,
    pub authority: Option<&'a Address>,
    pub freeze_authority: Option<&'a Address>,
    pub token_program: Option<&'a AccountView>,
}
