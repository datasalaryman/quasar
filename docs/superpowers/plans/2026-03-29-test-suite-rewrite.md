# Test Suite Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 5 Mollusk-based test files with 11 focused QuasarSVM-based files (127 tests) covering all happy paths and error paths with exact error code assertions.

**Architecture:** Each test file covers one concern (init, close, constraints, etc.). All tests use QuasarSVM via shared helpers in `helpers.rs`. Account state is constructed directly (not via chained instructions) where possible. The test-misc program (ID: `44444...`) and test-errors program (ID: `55555...`) provide the on-chain instructions.

**Tech Stack:** QuasarSVM, quasar-test-misc, quasar-test-errors, Rust `#[test]`

---

## File Structure

### Files to modify
- `tests/suite/src/helpers.rs` — add `svm_misc()`, `svm_errors()`, account constructors
- `tests/suite/src/lib.rs` — update module declarations

### Files to create (11 test files)
- `tests/suite/src/init.rs` — Account initialization (14 tests)
- `tests/suite/src/init_if_needed.rs` — Conditional init (10 tests)
- `tests/suite/src/close.rs` — Account closing (5 tests)
- `tests/suite/src/realloc.rs` — Account resizing (7 tests)
- `tests/suite/src/discriminator.rs` — Discriminator validation + NoDiscAccount (12 tests)
- `tests/suite/src/optional_accounts.rs` — Optional accounts + validation-when-present (7 tests)
- `tests/suite/src/constraints.rs` — has_one, address, constraint (19 tests)
- `tests/suite/src/account_validation.rs` — Account<T> owner/disc/size + SystemAccount + Program + UncheckedAccount (20 tests)
- `tests/suite/src/account_flags.rs` — Signer, mut, dup-allowed, double-mut (12 tests)
- `tests/suite/src/cpi_system.rs` — System CPI wrappers (10 tests)
- `tests/suite/src/errors.rs` — Error codes, require macros, default framework errors (11 tests)

### Files to delete
- `tests/suite/src/accounts.rs` — replaced by init, init_if_needed, close, realloc, discriminator, optional_accounts

(constraints.rs, account_validation.rs, cpi_system.rs, errors.rs are replaced by new versions)

---

## Constants & Error Codes Reference

### Account sizes
- `SimpleAccount` (disc=1): 42 bytes = 1 + 32(authority) + 8(value) + 1(bump)
- `MultiDiscAccount` (disc=[1,2]): 10 bytes = 2 + 8(data)
- `ErrorTestAccount` (disc=1): 41 bytes = 1 + 32(authority) + 8(value)
- `NoDiscAccount` (no disc): 40 bytes = 32(authority) + 8(value)

### QuasarError codes (Custom(N))
- 3000: AccountNotInitialized
- 3001: AccountAlreadyInitialized
- 3002: InvalidPda
- 3003: InvalidSeeds
- 3004: ConstraintViolation
- 3005: HasOneMismatch
- 3006: InvalidDiscriminator
- 3007: InsufficientSpace
- 3008: AccountNotRentExempt
- 3009: AccountOwnedByWrongProgram
- 3010: AccountNotMutable
- 3011: AccountNotSigner
- 3012: AddressMismatch

### test-errors TestError codes (Custom(N))
- 0: Hello
- 1: World
- 100: ExplicitNum
- 101: RequireFailed
- 102: RequireEqFailed
- 103: ConstraintCustom
- 104: AddressCustom

### test-misc TestError codes (Custom(N))
- 0: Unauthorized
- 1: InvalidAddress
- 2: CustomConstraint

---

## Task 0: Infrastructure

**Files:**
- Modify: `tests/suite/src/helpers.rs`
- Modify: `tests/suite/src/lib.rs`

- [ ] **Step 1: Update helpers.rs**

Add SVM factories and account constructors for test-misc and test-errors programs. Append after the existing content (keep all existing helpers for the quasar-spl tests).

```rust
// ---------------------------------------------------------------------------
// SVM factories — test-misc & test-errors
// ---------------------------------------------------------------------------

pub fn svm_misc() -> QuasarSvm {
    let elf = std::fs::read("../../target/deploy/quasar_test_misc.so").unwrap();
    QuasarSvm::new().with_program(&quasar_test_misc::ID, &elf)
}

pub fn svm_errors() -> QuasarSvm {
    let elf = std::fs::read("../../target/deploy/quasar_test_errors.so").unwrap();
    QuasarSvm::new().with_program(&quasar_test_errors::ID, &elf)
}

// ---------------------------------------------------------------------------
// Account constructors — test-misc state types
// ---------------------------------------------------------------------------

const SIMPLE_ACCOUNT_SIZE: usize = 42; // 1 disc + 32 addr + 8 u64 + 1 u8

/// Build raw data for SimpleAccount (disc=1).
pub fn build_simple_data(authority: Pubkey, value: u64, bump: u8) -> Vec<u8> {
    let mut data = vec![0u8; SIMPLE_ACCOUNT_SIZE];
    data[0] = 1;
    data[1..33].copy_from_slice(authority.as_ref());
    data[33..41].copy_from_slice(&value.to_le_bytes());
    data[41] = bump;
    data
}

/// Valid SimpleAccount owned by test-misc program.
pub fn simple_account(address: Pubkey, authority: Pubkey, value: u64, bump: u8) -> Account {
    raw_account(
        address,
        1_000_000,
        build_simple_data(authority, value, bump),
        quasar_test_misc::ID,
    )
}

const MULTI_DISC_SIZE: usize = 10; // 2 disc + 8 u64

/// Build raw data for MultiDiscAccount (disc=[1,2]).
pub fn build_multi_disc_data(value: u64) -> Vec<u8> {
    let mut data = vec![0u8; MULTI_DISC_SIZE];
    data[0] = 1;
    data[1] = 2;
    data[2..10].copy_from_slice(&value.to_le_bytes());
    data
}

/// Valid MultiDiscAccount owned by test-misc program.
pub fn multi_disc_account(address: Pubkey, value: u64) -> Account {
    raw_account(
        address,
        1_000_000,
        build_multi_disc_data(value),
        quasar_test_misc::ID,
    )
}

const ERROR_TEST_ACCOUNT_SIZE: usize = 41; // 1 disc + 32 addr + 8 u64

/// Build raw data for ErrorTestAccount (disc=1).
pub fn build_error_test_data(authority: Pubkey, value: u64) -> Vec<u8> {
    let mut data = vec![0u8; ERROR_TEST_ACCOUNT_SIZE];
    data[0] = 1;
    data[1..33].copy_from_slice(authority.as_ref());
    data[33..41].copy_from_slice(&value.to_le_bytes());
    data
}

/// Valid ErrorTestAccount owned by test-errors program.
pub fn error_test_account(address: Pubkey, authority: Pubkey, value: u64) -> Account {
    raw_account(
        address,
        1_000_000,
        build_error_test_data(authority, value),
        quasar_test_errors::ID,
    )
}

/// Account with custom lamports (for pre-funded init tests).
pub fn prefunded_account(address: Pubkey, lamports: u64) -> Account {
    Account {
        address,
        lamports,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

const NO_DISC_ACCOUNT_SIZE: usize = 40; // 32 addr + 8 u64 (no discriminator)

/// Build raw data for NoDiscAccount (unsafe_no_disc — no disc prefix).
pub fn build_no_disc_data(authority: Pubkey, value: u64) -> Vec<u8> {
    let mut data = vec![0u8; NO_DISC_ACCOUNT_SIZE];
    data[0..32].copy_from_slice(authority.as_ref());
    data[32..40].copy_from_slice(&value.to_le_bytes());
    data
}

/// Valid NoDiscAccount owned by test-misc program.
pub fn no_disc_account(address: Pubkey, authority: Pubkey, value: u64) -> Account {
    raw_account(
        address,
        1_000_000,
        build_no_disc_data(authority, value),
        quasar_test_misc::ID,
    )
}
```

- [ ] **Step 2: Update lib.rs**

Replace the old module declarations. Keep all existing QuasarSVM-based test modules (test_*, helpers, dynamic, events, header_tests, pda, remaining, sysvar, token_state). Remove accounts.rs. Add 13 new modules.

```rust
// Quasar Test Suite
//
// Integration tests for the Quasar Solana framework.
// Each module tests a specific concern via QuasarSVM.

#[cfg(test)]
mod dynamic;
#[cfg(test)]
mod events;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod pda;
#[cfg(test)]
mod remaining;
#[cfg(test)]
mod sysvar;
#[cfg(test)]
mod token_state;

// Core account lifecycle
#[cfg(test)]
mod init;
#[cfg(test)]
mod init_if_needed;
#[cfg(test)]
mod close;
#[cfg(test)]
mod realloc;
#[cfg(test)]
mod discriminator;
#[cfg(test)]
mod optional_accounts;

// Validation & constraints
#[cfg(test)]
mod constraints;
#[cfg(test)]
mod account_validation;
#[cfg(test)]
mod account_flags;

// CPI & errors
#[cfg(test)]
mod cpi_system;
#[cfg(test)]
mod errors;

// QuasarSVM-based SPL test suite
#[cfg(test)]
mod helpers;
#[cfg(test)]
mod test_ata_derivation;
#[cfg(test)]
mod test_close_attr;
#[cfg(test)]
mod test_cpi_approve_revoke;
#[cfg(test)]
mod test_cpi_close;
#[cfg(test)]
mod test_cpi_mint_burn;
#[cfg(test)]
mod test_cpi_transfer;
#[cfg(test)]
mod test_init_ata;
#[cfg(test)]
mod test_init_interface;
#[cfg(test)]
mod test_init_mint;
#[cfg(test)]
mod test_init_mint_pda;
#[cfg(test)]
mod test_init_token;
#[cfg(test)]
mod test_init_token_pda;
#[cfg(test)]
mod test_sweep;
#[cfg(test)]
mod test_validate_ata;
#[cfg(test)]
mod test_validate_mint;
#[cfg(test)]
mod test_validate_token;
```

- [ ] **Step 3: Run existing tests to verify no breakage**

```bash
cd tests/suite && cargo test 2>&1 | tail -5
```

Expected: All existing tests still pass (we only added new modules, haven't deleted old file yet).

- [ ] **Step 4: Commit infrastructure**

```bash
git add tests/suite/src/helpers.rs tests/suite/src/lib.rs
git commit -m "test: add QuasarSVM helpers for test-misc/test-errors programs"
```

---

## Task 1: init.rs — Account initialization (14 tests)

**Files:**
- Create: `tests/suite/src/init.rs`

**Test program:** quasar_test_misc — instructions: `initialize` (disc=0), `space_override` (disc=17), `explicit_payer` (disc=18)

**Account struct:** InitializeSimple { payer: &mut Signer, account: &mut Account<SimpleAccount>, system_program: Program<System> }
- Seeds: `[b"simple", payer]`

- [ ] **Step 1: Write init.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// Happy paths
// ============================================================================

#[test]
fn fresh_account() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_ok(), "fresh init: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data.len(), 42, "size");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(&acc.data[1..33], payer.as_ref(), "authority");
    assert_eq!(&acc.data[33..41], &42u64.to_le_bytes(), "value");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}

#[test]
fn prefunded_partial_rent() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 7,
    }
    .into();

    // Account has some lamports but less than rent-exempt minimum
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        prefunded_account(account, 500_000),
    ]);
    assert!(result.is_ok(), "prefunded partial: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}

#[test]
fn prefunded_excess_rent() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 7,
    }
    .into();

    // Account already has more than enough lamports
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        prefunded_account(account, 100_000_000),
    ]);
    assert!(result.is_ok(), "prefunded excess: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}

#[test]
fn after_close() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    // First init
    let ix1: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();
    let r1 = svm.process_instruction(&ix1, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(r1.is_ok(), "first init: {:?}", r1.raw_result);

    // Close
    let ix2: Instruction = CloseAccountInstruction {
        authority: payer,
        account,
    }
    .into();
    let r2 = svm.process_instruction(&ix2, &[]);
    assert!(r2.is_ok(), "close: {:?}", r2.raw_result);

    // Re-init
    let ix3: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 99,
    }
    .into();
    let r3 = svm.process_instruction(&ix3, &[]);
    assert!(r3.is_ok(), "re-init: {:?}", r3.raw_result);

    let acc = r3.account(&account).expect("account exists");
    assert_eq!(&acc.data[33..41], &99u64.to_le_bytes(), "new value");
}

#[test]
fn space_override() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"spacetest", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = SpaceOverrideInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_ok(), "space override: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data.len(), 100, "overridden space");
}

#[test]
fn explicit_payer() {
    let mut svm = svm_misc();
    let funder = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"explicit", funder.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = ExplicitPayerInstruction {
        funder,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(funder),
        empty_account(account),
    ]);
    assert!(result.is_ok(), "explicit payer: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(&acc.data[1..33], funder.as_ref(), "authority = funder");
}

// ============================================================================
// Error paths
// ============================================================================

#[test]
fn already_initialized() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Account already owned by program with valid data
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        simple_account(account, payer, 42, bump),
    ]);
    assert!(result.is_err(), "should reject already-initialized");
    result.assert_error(ProgramError::Custom(3001)); // AccountAlreadyInitialized
}

#[test]
fn owned_by_other_program() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Account owned by random program
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        raw_account(account, 1_000_000, vec![0u8; 42], Pubkey::new_unique()),
    ]);
    assert!(result.is_err(), "should reject account owned by other program");
    result.assert_error(ProgramError::Custom(3001)); // AccountAlreadyInitialized
}

#[test]
fn payer_not_signer() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let mut ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();
    ix.accounts[0].is_signer = false;

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_err(), "should reject non-signer payer");
    result.assert_error(ProgramError::MissingRequiredSignature);
}

#[test]
fn payer_insufficient_funds() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Payer with only 1 lamport
    let result = svm.process_instruction(&ix, &[
        Account {
            address: payer,
            lamports: 1,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        empty_account(account),
    ]);
    assert!(result.is_err(), "should reject insufficient funds");
}

#[test]
fn wrong_pda_seeds() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let wrong_account = Pubkey::new_unique(); // not a valid PDA

    let ix: Instruction = InitializeInstruction {
        payer,
        account: wrong_account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(wrong_account),
    ]);
    assert!(result.is_err(), "should reject wrong PDA");
}

#[test]
fn zero_data_owned_by_program() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // All-zero data but owned by our program
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        raw_account(account, 1_000_000, vec![0u8; 42], quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "should reject zero-data owned by program");
    result.assert_error(ProgramError::Custom(3001)); // AccountAlreadyInitialized
}

// ============================================================================
// Pre-funded edge cases
// ============================================================================

#[test]
fn prefunded_exact_no_topup() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let payer_lamports = 10_000_000_000u64;
    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 7,
    }
    .into();

    // Pre-fund with 10 SOL — well above rent-exempt minimum → no transfer CPI
    let result = svm.process_instruction(&ix, &[
        Account {
            address: payer,
            lamports: payer_lamports,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        prefunded_account(account, 100_000_000),
    ]);
    assert!(result.is_ok(), "prefunded no topup: {:?}", result.raw_result);

    // Verify payer was NOT charged (saturating_sub → required=0 → transfer skipped)
    let payer_after = result.account(&payer).expect("payer");
    assert_eq!(payer_after.lamports, payer_lamports, "payer not charged");
}

#[test]
fn prefunded_one_lamport() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 7,
    }
    .into();

    // Pre-fund with just 1 lamport — payer must top up almost all rent
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        prefunded_account(account, 1),
    ]);
    assert!(result.is_ok(), "prefunded 1 lamport: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}
```

- [ ] **Step 2: Run init tests**

```bash
cd tests/suite && cargo test init:: -- --nocapture 2>&1 | tail -20
```

Expected: All 14 tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/suite/src/init.rs
git commit -m "test(init): 14 tests covering fresh, prefunded, space override, error paths"
```

---

## Task 2: init_if_needed.rs (10 tests)

**Files:**
- Create: `tests/suite/src/init_if_needed.rs`

**Test program:** quasar_test_misc — instruction: `init_if_needed` (disc=7)

- [ ] **Step 1: Write init_if_needed.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// Happy paths
// ============================================================================

#[test]
fn new_account() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_ok(), "new account: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(&acc.data[33..41], &42u64.to_le_bytes(), "value");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}

#[test]
fn existing_valid() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 99,
    }
    .into();

    // Already initialized with correct owner/disc/size
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        simple_account(account, payer, 42, bump),
    ]);
    assert!(result.is_ok(), "existing valid: {:?}", result.raw_result);
}

#[test]
fn existing_value_updated() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 99,
    }
    .into();

    let payer_lamports_before = 100_000_000_000u64;
    let account_lamports_before = 1_000_000u64;
    let result = svm.process_instruction(&ix, &[
        Account {
            address: payer,
            lamports: payer_lamports_before,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        simple_account(account, payer, 42, bump),
    ]);
    assert!(result.is_ok(), "existing value: {:?}", result.raw_result);

    // Verify payer NOT charged (no init CPI happened)
    let payer_after = result.account(&payer).expect("payer");
    assert_eq!(payer_after.lamports, payer_lamports_before, "payer not charged");

    // Verify account lamports unchanged
    let acc = result.account(&account).expect("account");
    assert_eq!(acc.lamports, account_lamports_before, "account lamports unchanged");
}

#[test]
fn new_prefunded() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // System-owned with lamports → pre-funded init path
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        prefunded_account(account, 500_000),
    ]);
    assert!(result.is_ok(), "new prefunded: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account exists");
    assert_eq!(acc.data[0], 1, "discriminator");
    assert_eq!(acc.owner, quasar_test_misc::ID, "owner");
}

// ============================================================================
// Error paths — existing branch
// ============================================================================

#[test]
fn wrong_owner() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Owned by random program (not system, not ours)
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        raw_account(account, 1_000_000, vec![1u8; 42], Pubkey::new_unique()),
    ]);
    assert!(result.is_err(), "should reject wrong owner");
    result.assert_error(ProgramError::InvalidAccountOwner);
}

#[test]
fn wrong_discriminator() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Correct owner, wrong discriminator
    let mut data = vec![0u8; 42];
    data[0] = 99; // wrong disc
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "should reject wrong discriminator");
    result.assert_error(ProgramError::InvalidAccountData);
}

#[test]
fn data_too_small() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    // Correct owner + disc but data too small
    let mut data = vec![0u8; 10]; // too small (42 needed)
    data[0] = 1; // correct disc
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "should reject undersized data");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

#[test]
fn not_writable() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let mut ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();
    ix.accounts[1].is_writable = false;

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_err(), "should reject non-writable");
    result.assert_error(ProgramError::Immutable);
}

// ============================================================================
// Error paths — new branch
// ============================================================================

#[test]
fn payer_insufficient_funds() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        Account {
            address: payer,
            lamports: 1,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        empty_account(account),
    ]);
    assert!(result.is_err(), "should reject insufficient funds");
}

// ============================================================================
// Front-running scenario
// ============================================================================

#[test]
fn front_running_attacker_data() {
    // Attacker inits account with correct owner+disc but wrong field data
    // before legitimate user calls init_if_needed.
    // Since account is already initialized, init is skipped → validation passes
    // but data is attacker-controlled. This documents the known risk.
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let attacker = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitIfNeededInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 99,
    }
    .into();

    // Account already initialized by "attacker" — correct owner, disc, size
    // but authority = attacker (not payer)
    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        simple_account(account, attacker, 666, bump),
    ]);
    // Succeeds because existing account passes validation (owner+disc+size OK)
    assert!(result.is_ok(), "front-run: {:?}", result.raw_result);

    // Data retains attacker's values — init was skipped
    let acc = result.account(&account).expect("account");
    assert_eq!(&acc.data[1..33], attacker.as_ref(), "authority is attacker's");
}
```

- [ ] **Step 2: Run tests**

```bash
cd tests/suite && cargo test init_if_needed:: -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add tests/suite/src/init_if_needed.rs
git commit -m "test(init_if_needed): 10 tests for new/existing/prefunded/front-running paths"
```

---

## Task 3: close.rs (5 tests)

**Files:**
- Create: `tests/suite/src/close.rs`

- [ ] **Step 1: Write close.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

#[test]
fn success() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = CloseAccountInstruction { authority, account }.into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(authority),
        simple_account(account, authority, 42, bump),
    ]);
    assert!(result.is_ok(), "close: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account");
    assert_eq!(acc.lamports, 0, "lamports zeroed");
    assert_eq!(acc.owner, quasar_svm::system_program::ID, "owner reset to system");
    assert!(acc.data.is_empty() || acc.data.iter().all(|&b| b == 0), "data cleared");
}

#[test]
fn lamports_transferred() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let account_lamports = 2_000_000u64;
    let authority_lamports = 1_000_000u64;

    let ix: Instruction = CloseAccountInstruction { authority, account }.into();

    let result = svm.process_instruction(&ix, &[
        Account {
            address: authority,
            lamports: authority_lamports,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        Account {
            address: account,
            lamports: account_lamports,
            data: build_simple_data(authority, 42, bump),
            owner: quasar_test_misc::ID,
            executable: false,
        },
    ]);
    assert!(result.is_ok(), "close: {:?}", result.raw_result);

    let auth = result.account(&authority).expect("authority");
    assert_eq!(
        auth.lamports,
        authority_lamports + account_lamports,
        "authority receives exact lamports"
    );
}

#[test]
fn destination_balance_additive() {
    // Same as lamports_transferred but with explicit large balances
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let x = 50_000_000u64;
    let y = 3_000_000u64;

    let ix: Instruction = CloseAccountInstruction { authority, account }.into();

    let result = svm.process_instruction(&ix, &[
        Account {
            address: authority,
            lamports: x,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        Account {
            address: account,
            lamports: y,
            data: build_simple_data(authority, 42, bump),
            owner: quasar_test_misc::ID,
            executable: false,
        },
    ]);
    assert!(result.is_ok(), "close: {:?}", result.raw_result);

    let auth = result.account(&authority).expect("authority");
    assert_eq!(auth.lamports, x + y, "X + Y additive");
}

#[test]
fn wrong_authority() {
    let mut svm = svm_misc();
    let real_authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", real_authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = CloseAccountInstruction {
        authority: wrong_authority,
        account,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(wrong_authority),
        simple_account(account, real_authority, 42, bump),
    ]);
    assert!(result.is_err(), "should reject wrong authority");
    result.assert_error(ProgramError::Custom(3005)); // HasOneMismatch
}

#[test]
fn authority_not_signer() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let mut ix: Instruction = CloseAccountInstruction { authority, account }.into();
    ix.accounts[0].is_signer = false;

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(authority),
        simple_account(account, authority, 42, bump),
    ]);
    assert!(result.is_err(), "should reject non-signer authority");
    result.assert_error(ProgramError::MissingRequiredSignature);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test close:: -- --nocapture 2>&1 | tail -10
git add tests/suite/src/close.rs && git commit -m "test(close): 5 tests for close lifecycle"
```

---

## Task 4: realloc.rs (7 tests)

**Files:**
- Create: `tests/suite/src/realloc.rs`

- [ ] **Step 1: Write realloc.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey},
    quasar_test_misc::cpi::*,
};

fn setup_account(svm: &mut quasar_svm::QuasarSvm) -> (Pubkey, Pubkey, Pubkey) {
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"simple", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    // Init first
    let ix: Instruction = InitializeInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();
    let r = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(r.is_ok(), "setup init: {:?}", r.raw_result);
    (payer, account, quasar_svm::system_program::ID)
}

fn realloc(svm: &mut quasar_svm::QuasarSvm, account: Pubkey, payer: Pubkey, new_space: u64) -> quasar_svm::ExecutionResult {
    let ix: Instruction = ReallocCheckInstruction {
        account,
        payer,
        system_program: quasar_svm::system_program::ID,
        new_space,
    }
    .into();
    svm.process_instruction(&ix, &[])
}

#[test]
fn grow() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);
    let result = realloc(&mut svm, account, payer, 100);
    assert!(result.is_ok(), "grow: {:?}", result.raw_result);
    let acc = result.account(&account).expect("account");
    assert_eq!(acc.data.len(), 100, "new length");
}

#[test]
fn grow_preserves_data() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);

    // Read original data
    let orig = svm.get_account(&account).expect("account").data.clone();

    let result = realloc(&mut svm, account, payer, 100);
    assert!(result.is_ok(), "grow: {:?}", result.raw_result);
    let acc = result.account(&account).expect("account");
    assert_eq!(&acc.data[..42], &orig[..], "original 42 bytes preserved");
}

#[test]
fn shrink() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);

    // First grow
    let r1 = realloc(&mut svm, account, payer, 100);
    assert!(r1.is_ok(), "grow: {:?}", r1.raw_result);

    // Then shrink
    let r2 = realloc(&mut svm, account, payer, 42);
    assert!(r2.is_ok(), "shrink: {:?}", r2.raw_result);
    let acc = r2.account(&account).expect("account");
    assert_eq!(acc.data.len(), 42, "shrunk back");
}

#[test]
fn same_size_noop() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);
    let result = realloc(&mut svm, account, payer, 42);
    assert!(result.is_ok(), "noop: {:?}", result.raw_result);
    let acc = result.account(&account).expect("account");
    assert_eq!(acc.data.len(), 42, "unchanged");
}

#[test]
fn grow_large() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);
    let result = realloc(&mut svm, account, payer, 10_000);
    assert!(result.is_ok(), "grow large: {:?}", result.raw_result);
    let acc = result.account(&account).expect("account");
    assert_eq!(acc.data.len(), 10_000, "large size");
}

#[test]
fn grow_zeroes_new_region() {
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);

    // Grow to 100
    let r1 = realloc(&mut svm, account, payer, 100);
    assert!(r1.is_ok());

    // Shrink to 42
    let r2 = realloc(&mut svm, account, payer, 42);
    assert!(r2.is_ok());

    // Grow to 100 again — new region should be zeroed
    let r3 = realloc(&mut svm, account, payer, 100);
    assert!(r3.is_ok(), "re-grow: {:?}", r3.raw_result);
    let acc = r3.account(&account).expect("account");
    assert!(
        acc.data[42..100].iter().all(|&b| b == 0),
        "re-grown region must be zeroed (no data leak)"
    );
}

#[test]
fn shrink_below_disc_then_read_fails() {
    // Shrink account below discriminator size, then attempt to validate —
    // the discriminator bytes get zeroed by shrink, so subsequent reads
    // should reject the account.
    let mut svm = svm_misc();
    let (payer, account, _) = setup_account(&mut svm);

    // Shrink to 4 bytes (disc is 1 byte for SimpleAccount, so this truncates fields)
    let r1 = realloc(&mut svm, account, payer, 4);
    assert!(r1.is_ok(), "shrink to 4: {:?}", r1.raw_result);

    let acc = r1.account(&account).expect("account");
    assert_eq!(acc.data.len(), 4, "shrunk to 4 bytes");

    // Now try to use the account via owner_check (which validates Account<SimpleAccount>)
    let ix: Instruction = quasar_test_misc::cpi::OwnerCheckInstruction { account }.into();
    let r2 = svm.process_instruction(&ix, &[]);
    assert!(r2.is_err(), "should reject shrunk account");
    // Account data is too small for SimpleAccount (42 bytes needed)
    r2.assert_error(ProgramError::AccountDataTooSmall);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test realloc:: -- --nocapture 2>&1 | tail -10
git add tests/suite/src/realloc.rs && git commit -m "test(realloc): 7 tests for grow/shrink/noop/disc-corruption"
```

---

## Task 5: discriminator.rs (12 tests)

**Files:**
- Create: `tests/suite/src/discriminator.rs`

- [ ] **Step 1: Write discriminator.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// Happy paths
// ============================================================================

#[test]
fn single_byte_valid() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, authority, 42, 0),
    ]);
    assert!(result.is_ok(), "single byte disc: {:?}", result.raw_result);
}

#[test]
fn multi_byte_valid() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = CheckMultiDiscInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        multi_disc_account(account, 42),
    ]);
    assert!(result.is_ok(), "multi byte disc: {:?}", result.raw_result);
}

// ============================================================================
// Error paths
// ============================================================================

#[test]
fn single_byte_wrong() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 42];
    data[0] = 2; // wrong disc (expected 1)

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "wrong single-byte disc");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}

#[test]
fn single_byte_zero() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let data = vec![0u8; 42]; // disc = 0 (uninitialized)

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "zero disc");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}

#[test]
fn multi_byte_partial_match() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 10];
    data[0] = 1; // first byte correct
    data[1] = 0; // second byte wrong (expected 2)

    let ix: Instruction = CheckMultiDiscInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "partial disc match");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}

#[test]
fn multi_byte_reversed() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 10];
    data[0] = 2; // swapped
    data[1] = 1; // swapped

    let ix: Instruction = CheckMultiDiscInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "reversed disc");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}

#[test]
fn zero_length_data() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, vec![], quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "zero length data");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

#[test]
fn disc_only_no_fields() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let data = vec![1u8]; // just the disc, no struct fields

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "disc only, no fields");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

#[test]
fn oversized_data_valid() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let mut data = vec![0u8; 10_000];
    data[0] = 1; // correct disc
    data[1..33].copy_from_slice(authority.as_ref());
    data[33..41].copy_from_slice(&42u64.to_le_bytes());
    data[41] = 0; // bump

    let ix: Instruction = OwnerCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 100_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_ok(), "oversized data should be accepted: {:?}", result.raw_result);
}

// ============================================================================
// NoDiscAccount (unsafe_no_disc) — no discriminator check at all
// ============================================================================

#[test]
fn no_disc_init_success() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"nodisc", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = InitNoDiscInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 42,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(result.is_ok(), "no_disc init: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account");
    assert_eq!(acc.data.len(), 40, "no-disc size (no disc prefix)");
    assert_eq!(&acc.data[0..32], payer.as_ref(), "authority");
    assert_eq!(&acc.data[32..40], &42u64.to_le_bytes(), "value");
}

#[test]
fn no_disc_read_after_init() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let (account, _bump) = Pubkey::find_program_address(
        &[b"nodisc", payer.as_ref()],
        &quasar_test_misc::ID,
    );

    // Init first
    let ix1: Instruction = InitNoDiscInstruction {
        payer,
        account,
        system_program: quasar_svm::system_program::ID,
        value: 99,
    }
    .into();
    let r1 = svm.process_instruction(&ix1, &[
        rich_signer_account(payer),
        empty_account(account),
    ]);
    assert!(r1.is_ok(), "init: {:?}", r1.raw_result);

    // Read — handler accesses .authority and .value via Deref
    let ix2: Instruction = ReadNoDiscInstruction { account }.into();
    let r2 = svm.process_instruction(&ix2, &[]);
    assert!(r2.is_ok(), "read: {:?}", r2.raw_result);
}

#[test]
fn no_disc_any_data_accepted() {
    // Since unsafe_no_disc skips discriminator check, any 40+ byte data
    // owned by the program should pass validation.
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    // All 0xFF data — no valid discriminator, but unsafe_no_disc skips the check
    let data = vec![0xFF; 40];
    let ix: Instruction = ReadNoDiscInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_misc::ID),
    ]);
    assert!(result.is_ok(), "any data accepted: {:?}", result.raw_result);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test discriminator:: -- --nocapture 2>&1 | tail -15
git add tests/suite/src/discriminator.rs && git commit -m "test(discriminator): 12 tests for single/multi-byte/no-disc validation"
```

---

## Task 6: optional_accounts.rs (7 tests)

**Files:**
- Create: `tests/suite/src/optional_accounts.rs`

- [ ] **Step 1: Write optional_accounts.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

#[test]
fn some_valid() {
    let mut svm = svm_misc();
    let required = Pubkey::new_unique();
    let optional = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = OptionalAccountInstruction {
        required,
        optional,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        simple_account(required, authority, 42, 0),
        simple_account(optional, authority, 99, 0),
    ]);
    assert!(result.is_ok(), "both present: {:?}", result.raw_result);
}

#[test]
fn none_sentinel() {
    let mut svm = svm_misc();
    let required = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    // Sentinel = program ID for None
    let sentinel = quasar_test_misc::ID;
    let ix: Instruction = OptionalAccountInstruction {
        required,
        optional: sentinel,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        simple_account(required, authority, 42, 0),
    ]);
    assert!(result.is_ok(), "sentinel none: {:?}", result.raw_result);
}

#[test]
fn has_one_some_valid() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = OptionalHasOneInstruction { authority, account }.into();

    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, authority, 42, 0),
    ]);
    assert!(result.is_ok(), "has_one some valid: {:?}", result.raw_result);
}

#[test]
fn has_one_none_skipped() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let sentinel = quasar_test_misc::ID;

    let ix: Instruction = OptionalHasOneInstruction {
        authority,
        account: sentinel,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
    ]);
    assert!(result.is_ok(), "has_one none skipped: {:?}", result.raw_result);
}

#[test]
fn has_one_some_wrong_authority() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = OptionalHasOneInstruction { authority, account }.into();

    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, wrong_authority, 42, 0), // wrong authority stored
    ]);
    assert!(result.is_err(), "should reject wrong authority on present optional");
    result.assert_error(ProgramError::Custom(3005)); // HasOneMismatch
}

// ============================================================================
// Validation still runs when present (not sentinel)
// ============================================================================

#[test]
fn some_wrong_owner() {
    // Optional account is present (not sentinel) but owned by wrong program.
    // Proves Optional doesn't skip validation when the account IS present.
    let mut svm = svm_misc();
    let required = Pubkey::new_unique();
    let optional = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = OptionalAccountInstruction {
        required,
        optional,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        simple_account(required, authority, 42, 0),
        raw_account(
            optional,
            1_000_000,
            build_simple_data(authority, 99, 0),
            Pubkey::new_unique(), // wrong owner
        ),
    ]);
    assert!(result.is_err(), "wrong owner on present optional");
    result.assert_error(ProgramError::Custom(3009)); // AccountOwnedByWrongProgram
}

#[test]
fn some_wrong_discriminator() {
    // Optional account is present but has wrong discriminator.
    let mut svm = svm_misc();
    let required = Pubkey::new_unique();
    let optional = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = OptionalAccountInstruction {
        required,
        optional,
    }
    .into();

    let mut bad_data = vec![0u8; 42];
    bad_data[0] = 99; // wrong disc
    let result = svm.process_instruction(&ix, &[
        simple_account(required, authority, 42, 0),
        raw_account(optional, 1_000_000, bad_data, quasar_test_misc::ID),
    ]);
    assert!(result.is_err(), "wrong disc on present optional");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test optional_accounts:: -- --nocapture 2>&1 | tail -10
git add tests/suite/src/optional_accounts.rs && git commit -m "test(optional): 7 tests for Some/None/has_one/validation-when-present"
```

---

## Task 7: constraints.rs (19 tests)

**Files:**
- Create: `tests/suite/src/constraints.rs`

- [ ] **Step 1: Write constraints.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// has_one — default error
// ============================================================================

#[test]
fn has_one_success() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = UpdateHasOneInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, authority, 42, bump),
    ]);
    assert!(result.is_ok(), "has_one: {:?}", result.raw_result);
}

#[test]
fn has_one_mismatch() {
    let mut svm = svm_misc();
    let real_authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", real_authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = UpdateHasOneInstruction {
        authority: wrong_authority,
        account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(wrong_authority),
        simple_account(account, real_authority, 42, bump),
    ]);
    assert!(result.is_err(), "has_one mismatch");
    result.assert_error(ProgramError::Custom(3005)); // HasOneMismatch
}

#[test]
fn has_one_zeroed_authority() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let zero_authority = Pubkey::default();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    let ix: Instruction = UpdateHasOneInstruction { authority, account }.into();
    // Stored authority is zeroed
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, zero_authority, 42, bump),
    ]);
    assert!(result.is_err(), "zeroed stored authority should fail");
    result.assert_error(ProgramError::Custom(3005));
}

#[test]
fn has_one_single_bit_diff() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    // XOR bit 0 of stored authority
    let mut bad_bytes = authority.to_bytes();
    bad_bytes[0] ^= 1;
    let bad_authority = Pubkey::from(bad_bytes);

    let ix: Instruction = UpdateHasOneInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, bad_authority, 42, bump),
    ]);
    assert!(result.is_err(), "single bit diff");
    result.assert_error(ProgramError::Custom(3005));
}

#[test]
fn has_one_last_byte_diff() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", authority.as_ref()],
        &quasar_test_misc::ID,
    );

    // XOR byte 31
    let mut bad_bytes = authority.to_bytes();
    bad_bytes[31] ^= 0xFF;
    let bad_authority = Pubkey::from(bad_bytes);

    let ix: Instruction = UpdateHasOneInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, bad_authority, 42, bump),
    ]);
    assert!(result.is_err(), "last byte diff");
    result.assert_error(ProgramError::Custom(3005));
}

#[test]
fn has_one_default_passed() {
    let mut svm = svm_misc();
    let real_authority = Pubkey::new_unique();
    let default_authority = Pubkey::default();
    let (account, bump) = Pubkey::find_program_address(
        &[b"simple", default_authority.as_ref()],
        &quasar_test_misc::ID,
    );

    // Passed authority = default, stored = real
    let ix: Instruction = UpdateHasOneInstruction {
        authority: default_authority,
        account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(default_authority),
        simple_account(account, real_authority, 42, bump),
    ]);
    assert!(result.is_err(), "default authority passed");
    result.assert_error(ProgramError::Custom(3005));
}

// ============================================================================
// has_one — custom error (via test-errors crate)
// ============================================================================

#[test]
fn has_one_custom_success() {
    let mut svm = svm_errors();
    let authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::HasOneCustomInstruction {
        authority,
        account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        error_test_account(account, authority, 42),
    ]);
    assert!(result.is_ok(), "has_one custom success: {:?}", result.raw_result);
}

#[test]
fn has_one_custom_mismatch() {
    let mut svm = svm_errors();
    let authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::HasOneCustomInstruction {
        authority,
        account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        error_test_account(account, wrong_authority, 42),
    ]);
    assert!(result.is_err(), "has_one custom mismatch");
    result.assert_error(ProgramError::Custom(0)); // TestError::Hello
}

// ============================================================================
// address — default error
// ============================================================================

#[test]
fn address_success() {
    let mut svm = svm_misc();
    let expected: Pubkey = Pubkey::from([42u8; 32]); // EXPECTED_ADDRESS in test-misc

    let ix: Instruction = UpdateAddressInstruction { target: expected }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(expected, Pubkey::new_unique(), 42, 0),
    ]);
    assert!(result.is_ok(), "address match: {:?}", result.raw_result);
}

#[test]
fn address_mismatch() {
    let mut svm = svm_misc();
    let wrong = Pubkey::new_unique();

    let ix: Instruction = UpdateAddressInstruction { target: wrong }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(wrong, Pubkey::new_unique(), 42, 0),
    ]);
    assert!(result.is_err(), "address mismatch");
    result.assert_error(ProgramError::Custom(3012)); // AddressMismatch
}

// ============================================================================
// address — custom error (via test-errors crate)
// ============================================================================

#[test]
fn address_custom_success() {
    let mut svm = svm_errors();
    let expected: Pubkey = Pubkey::from([99u8; 32]); // EXPECTED_ADDR in test-errors

    let ix: Instruction =
        quasar_test_errors::cpi::AddressCustomErrorInstruction { target: expected }.into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(expected, Pubkey::new_unique(), 42),
    ]);
    assert!(result.is_ok(), "address custom: {:?}", result.raw_result);
}

#[test]
fn address_custom_mismatch() {
    let mut svm = svm_errors();
    let wrong = Pubkey::new_unique();

    let ix: Instruction =
        quasar_test_errors::cpi::AddressCustomErrorInstruction { target: wrong }.into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(wrong, Pubkey::new_unique(), 42),
    ]);
    assert!(result.is_err(), "address custom mismatch");
    result.assert_error(ProgramError::Custom(104)); // TestError::AddressCustom
}

// ============================================================================
// constraint — default error
// ============================================================================

#[test]
fn constraint_success() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = ConstraintCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 100, 0), // value > 0
    ]);
    assert!(result.is_ok(), "constraint pass: {:?}", result.raw_result);
}

#[test]
fn constraint_fail() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = ConstraintCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 0, 0), // value == 0
    ]);
    assert!(result.is_err(), "constraint fail");
    result.assert_error(ProgramError::Custom(3004)); // ConstraintViolation
}

// ============================================================================
// constraint — custom error
// ============================================================================

#[test]
fn constraint_custom_success() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = ConstraintCustomErrorInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 100, 0),
    ]);
    assert!(result.is_ok(), "constraint custom pass: {:?}", result.raw_result);
}

#[test]
fn constraint_custom_fail() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = ConstraintCustomErrorInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 0, 0),
    ]);
    assert!(result.is_err(), "constraint custom fail");
    result.assert_error(ProgramError::Custom(2)); // TestError::CustomConstraint
}

// ============================================================================
// combined constraints
// ============================================================================

#[test]
fn has_one_and_owner_success() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = HasOneAndOwnerCheckInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, authority, 42, 0),
    ]);
    assert!(result.is_ok(), "combined: {:?}", result.raw_result);
}

#[test]
fn has_one_and_owner_wrong_authority() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = HasOneAndOwnerCheckInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        simple_account(account, wrong_authority, 42, 0),
    ]);
    assert!(result.is_err(), "wrong authority");
    result.assert_error(ProgramError::Custom(3005)); // HasOneMismatch
}

#[test]
fn has_one_and_owner_wrong_owner() {
    let mut svm = svm_misc();
    let authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = HasOneAndOwnerCheckInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        raw_account(
            account,
            1_000_000,
            build_simple_data(authority, 42, 0),
            Pubkey::new_unique(), // wrong owner
        ),
    ]);
    assert!(result.is_err(), "wrong owner");
    result.assert_error(ProgramError::Custom(3009)); // AccountOwnedByWrongProgram
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test constraints:: -- --nocapture 2>&1 | tail -25
git add tests/suite/src/constraints.rs && git commit -m "test(constraints): 19 tests for has_one/address/constraint"
```

---

## Task 8: account_validation.rs (20 tests)

**Files:**
- Create: `tests/suite/src/account_validation.rs`

**Test programs:**
- quasar_test_errors — `account_check` (disc=9), `mut_account_check` (disc=10), `two_accounts_check` (disc=24), `system_account_check` (disc=20), `program_check` (disc=21), `unchecked_account_check` (disc=23)

- [ ] **Step 1: Write account_validation.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_errors::cpi::*,
};

// ============================================================================
// Happy paths
// ============================================================================

#[test]
fn valid_account() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(account, authority, 42),
    ]);
    assert!(result.is_ok(), "valid: {:?}", result.raw_result);
}

#[test]
fn valid_with_extra_data() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let mut data = build_error_test_data(authority, 42);
    data.extend_from_slice(&[0u8; 100]); // extra bytes

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_errors::ID),
    ]);
    // Current behavior: oversized data is accepted
    assert!(result.is_ok(), "extra data: {:?}", result.raw_result);
}

// ============================================================================
// Owner checks
// ============================================================================

#[test]
fn wrong_owner() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(
            account,
            1_000_000,
            build_error_test_data(authority, 42),
            Pubkey::new_unique(), // wrong owner
        ),
    ]);
    assert!(result.is_err(), "wrong owner");
    result.assert_error(ProgramError::Custom(3009)); // AccountOwnedByWrongProgram
}

#[test]
fn system_program_owner() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(
            account,
            1_000_000,
            build_error_test_data(authority, 42),
            quasar_svm::system_program::ID,
        ),
    ]);
    assert!(result.is_err(), "system program owner");
    result.assert_error(ProgramError::Custom(3009));
}

// ============================================================================
// Discriminator checks
// ============================================================================

#[test]
fn wrong_discriminator() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 41];
    data[0] = 99; // wrong disc

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "wrong discriminator");
    result.assert_error(ProgramError::Custom(3006)); // InvalidDiscriminator
}

#[test]
fn zero_discriminator() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let data = vec![0u8; 41]; // disc = 0

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "zero discriminator");
    result.assert_error(ProgramError::Custom(3006));
}

// ============================================================================
// Size checks
// ============================================================================

#[test]
fn data_too_small() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 20]; // 41 needed
    data[0] = 1; // correct disc

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "data too small");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

#[test]
fn empty_data() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, vec![], quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "empty data");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

#[test]
fn one_byte_short() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let mut data = vec![0u8; 40]; // 41 needed
    data[0] = 1;

    let ix: Instruction = AccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, data, quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "one byte short");
    result.assert_error(ProgramError::AccountDataTooSmall);
}

// ============================================================================
// Duplicate detection
// ============================================================================

#[test]
fn duplicate_same_address() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    // Two accounts with same address
    let ix: Instruction = TwoAccountsCheckInstruction {
        first: account,
        second: account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(account, authority, 42),
    ]);
    assert!(result.is_err(), "duplicate should fail");
}

#[test]
fn two_distinct_accounts() {
    let mut svm = svm_errors();
    let first = Pubkey::new_unique();
    let second = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = TwoAccountsCheckInstruction { first, second }.into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(first, authority, 42),
        error_test_account(second, authority, 99),
    ]);
    assert!(result.is_ok(), "distinct accounts: {:?}", result.raw_result);
}

// ============================================================================
// SystemAccount validation (merged from system_account.rs)
// ============================================================================

#[test]
fn system_account_success() {
    let mut svm = svm_errors();
    let target = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::SystemAccountCheckInstruction { account: target }.into();
    let result = svm.process_instruction(&ix, &[signer_account(target)]);
    assert!(result.is_ok(), "system account: {:?}", result.raw_result);
}

#[test]
fn system_account_wrong_owner() {
    let mut svm = svm_errors();
    let target = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::SystemAccountCheckInstruction { account: target }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(target, 1_000_000, vec![], Pubkey::new_unique()),
    ]);
    assert!(result.is_err(), "wrong owner");
    result.assert_error(ProgramError::Custom(3009)); // AccountOwnedByWrongProgram
}

#[test]
fn system_account_owned_by_program() {
    let mut svm = svm_errors();
    let target = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::SystemAccountCheckInstruction { account: target }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(target, 1_000_000, vec![], quasar_test_errors::ID),
    ]);
    assert!(result.is_err(), "owned by program");
    result.assert_error(ProgramError::Custom(3009));
}

// ============================================================================
// Program<T> validation (merged from program_check.rs)
// ============================================================================

#[test]
fn program_success() {
    let mut svm = svm_errors();
    let program = quasar_svm::system_program::ID;

    let ix: Instruction = ProgramCheckInstruction { program }.into();
    let result = svm.process_instruction(&ix, &[]);
    assert!(result.is_ok(), "program check: {:?}", result.raw_result);
}

#[test]
fn program_wrong_id() {
    let mut svm = svm_errors();
    let wrong = Pubkey::new_unique();

    let ix: Instruction = ProgramCheckInstruction { program: wrong }.into();
    let result = svm.process_instruction(&ix, &[
        Account {
            address: wrong,
            lamports: 1_000_000,
            data: vec![],
            owner: Pubkey::default(),
            executable: true,
        },
    ]);
    assert!(result.is_err(), "wrong program ID");
    result.assert_error(ProgramError::IncorrectProgramId);
}

#[test]
fn program_not_executable() {
    let mut svm = svm_errors();
    let system = quasar_svm::system_program::ID;

    let ix: Instruction = ProgramCheckInstruction { program: system }.into();
    let result = svm.process_instruction(&ix, &[
        Account {
            address: system,
            lamports: 1,
            data: vec![],
            owner: Pubkey::default(),
            executable: false,
        },
    ]);
    assert!(result.is_err(), "not executable");
    result.assert_error(ProgramError::InvalidAccountData);
}

// ============================================================================
// UncheckedAccount — verifies NO validation is applied
// ============================================================================

#[test]
fn unchecked_any_owner_passes() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();

    let ix: Instruction = UncheckedAccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 1_000_000, vec![1, 2, 3], Pubkey::new_unique()),
    ]);
    assert!(result.is_ok(), "unchecked any owner: {:?}", result.raw_result);
}

#[test]
fn unchecked_empty_passes() {
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();

    let ix: Instruction = UncheckedAccountCheckInstruction { account }.into();
    let result = svm.process_instruction(&ix, &[
        raw_account(account, 0, vec![], quasar_svm::system_program::ID),
    ]);
    assert!(result.is_ok(), "unchecked empty: {:?}", result.raw_result);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test account_validation:: -- --nocapture 2>&1 | tail -20
git add tests/suite/src/account_validation.rs && git commit -m "test(validation): 20 tests for Account<T>/SystemAccount/Program/UncheckedAccount"
```

---

## Task 9: account_flags.rs (12 tests)

**Files:**
- Create: `tests/suite/src/account_flags.rs`

- [ ] **Step 1: Write account_flags.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Account, Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// Signer
// ============================================================================

#[test]
fn signer_success() {
    let mut svm = svm_misc();
    let signer = Pubkey::new_unique();

    let ix: Instruction = SignerCheckInstruction { signer }.into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_ok(), "signer: {:?}", result.raw_result);
}

#[test]
fn signer_not_signer() {
    let mut svm = svm_misc();
    let signer = Pubkey::new_unique();

    let mut ix: Instruction = SignerCheckInstruction { signer }.into();
    ix.accounts[0].is_signer = false;

    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err(), "not signer");
    result.assert_error(ProgramError::MissingRequiredSignature);
}

// ============================================================================
// Mut
// ============================================================================

#[test]
fn mut_success() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = MutCheckInstruction {
        account,
        new_value: 99,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 42, 0),
    ]);
    assert!(result.is_ok(), "mut: {:?}", result.raw_result);
}

#[test]
fn mut_write_persists() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = MutCheckInstruction {
        account,
        new_value: 99,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, authority, 42, 0),
    ]);
    assert!(result.is_ok(), "mut write: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account");
    assert_eq!(&acc.data[33..41], &99u64.to_le_bytes(), "written value persisted");
}

#[test]
fn mut_not_writable() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let mut ix: Instruction = MutCheckInstruction {
        account,
        new_value: 99,
    }
    .into();
    ix.accounts[0].is_writable = false;

    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 42, 0),
    ]);
    assert!(result.is_err(), "not writable");
    result.assert_error(ProgramError::Immutable);
}

// ============================================================================
// Combined signer + mut
// ============================================================================

#[test]
fn signer_and_mut_success() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let signer = Pubkey::new_unique();

    let ix: Instruction = SignerAndMutCheckInstruction {
        account,
        signer,
        new_value: 99,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 42, 0),
        signer_account(signer),
    ]);
    assert!(result.is_ok(), "signer+mut: {:?}", result.raw_result);
}

#[test]
fn signer_and_mut_missing_signer() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let signer = Pubkey::new_unique();

    let mut ix: Instruction = SignerAndMutCheckInstruction {
        account,
        signer,
        new_value: 99,
    }
    .into();
    ix.accounts[1].is_signer = false;

    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 42, 0),
        signer_account(signer),
    ]);
    assert!(result.is_err(), "missing signer");
    result.assert_error(ProgramError::MissingRequiredSignature);
}

#[test]
fn signer_and_mut_not_writable() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let signer = Pubkey::new_unique();

    let mut ix: Instruction = SignerAndMutCheckInstruction {
        account,
        signer,
        new_value: 99,
    }
    .into();
    ix.accounts[0].is_writable = false;

    let result = svm.process_instruction(&ix, &[
        simple_account(account, Pubkey::new_unique(), 42, 0),
        signer_account(signer),
    ]);
    assert!(result.is_err(), "not writable");
    result.assert_error(ProgramError::Immutable);
}

// ============================================================================
// Dup-allowed path (#[account(dup)]) — separate codegen from nodup
// ============================================================================

#[test]
fn dup_mut_same_account_succeeds() {
    // HeaderDupMut: source=Signer, destination=dup mut UncheckedAccount
    // Same pubkey for both should succeed because destination has #[account(dup)]
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::HeaderDupMutInstruction {
        source: account,
        destination: account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(account)]);
    assert!(result.is_ok(), "dup mut same account: {:?}", result.raw_result);
}

#[test]
fn dup_signer_same_account_succeeds() {
    // HeaderDupSigner: payer=mut Signer, authority=dup Signer
    // Same pubkey for both should succeed because authority has #[account(dup)]
    let mut svm = svm_errors();
    let account = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::HeaderDupSignerInstruction {
        payer: account,
        authority: account,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(account)]);
    assert!(result.is_ok(), "dup signer same account: {:?}", result.raw_result);
}

#[test]
fn three_accounts_no_dup_rejects_same() {
    // ThreeAccountsDup: Signer + mut UncheckedAccount + UncheckedAccount
    // NO #[account(dup)] — so second==third must be rejected by nodup check
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();
    let shared = Pubkey::new_unique();

    let ix: Instruction = quasar_test_errors::cpi::ThreeAccountsDupInstruction {
        first: signer,
        second: shared,
        third: shared, // same as second
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(signer),
        signer_account(shared), // just needs some account
    ]);
    assert!(result.is_err(), "should reject dup without #[account(dup)]");
}

// ============================================================================
// Double mut — two separate &mut fields in one instruction
// ============================================================================

#[test]
fn double_mut_distinct_accounts() {
    let mut svm = svm_misc();
    let signer = Pubkey::new_unique();
    let a = Pubkey::new_unique();
    let b = Pubkey::new_unique();
    let authority = Pubkey::new_unique();

    let ix: Instruction = DoubleMutCheckInstruction {
        signer,
        account_a: a,
        account_b: b,
    }
    .into();
    let result = svm.process_instruction(&ix, &[
        signer_account(signer),
        simple_account(a, authority, 42, 0),
        simple_account(b, authority, 99, 0),
    ]);
    assert!(result.is_ok(), "double mut: {:?}", result.raw_result);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test account_flags:: -- --nocapture 2>&1 | tail -15
git add tests/suite/src/account_flags.rs && git commit -m "test(flags): 12 tests for signer/mut/dup-allowed/double-mut"
```

---

## Task 10: cpi_system.rs (10 tests)

**Files:**
- Create: `tests/suite/src/cpi_system.rs`

- [ ] **Step 1: Write cpi_system.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_misc::cpi::*,
};

// ============================================================================
// create_account
// ============================================================================

#[test]
fn create_success() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let new_account = Pubkey::new_unique();
    let owner = Pubkey::new_unique();

    let ix: Instruction = CreateAccountTestInstruction {
        payer,
        new_account,
        system_program: quasar_svm::system_program::ID,
        lamports: 1_000_000,
        space: 100,
        owner,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        signer_account(new_account),
    ]);
    assert!(result.is_ok(), "create: {:?}", result.raw_result);

    let acc = result.account(&new_account).expect("created account");
    assert_eq!(acc.data.len(), 100, "space");
    assert_eq!(acc.owner, owner, "owner");
    assert!(acc.lamports >= 1_000_000, "lamports");
}

#[test]
fn create_zero_space() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let new_account = Pubkey::new_unique();
    let owner = Pubkey::new_unique();

    let ix: Instruction = CreateAccountTestInstruction {
        payer,
        new_account,
        system_program: quasar_svm::system_program::ID,
        lamports: 890_880, // rent for 0 bytes
        space: 0,
        owner,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        signer_account(new_account),
    ]);
    assert!(result.is_ok(), "zero space: {:?}", result.raw_result);
}

#[test]
fn create_large_space() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let new_account = Pubkey::new_unique();
    let owner = Pubkey::new_unique();

    let ix: Instruction = CreateAccountTestInstruction {
        payer,
        new_account,
        system_program: quasar_svm::system_program::ID,
        lamports: 100_000_000,
        space: 10_000,
        owner,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(payer),
        signer_account(new_account),
    ]);
    assert!(result.is_ok(), "large space: {:?}", result.raw_result);
    let acc = result.account(&new_account).expect("account");
    assert_eq!(acc.data.len(), 10_000);
}

#[test]
fn create_insufficient_funds() {
    let mut svm = svm_misc();
    let payer = Pubkey::new_unique();
    let new_account = Pubkey::new_unique();

    let ix: Instruction = CreateAccountTestInstruction {
        payer,
        new_account,
        system_program: quasar_svm::system_program::ID,
        lamports: 100_000_000,
        space: 100,
        owner: Pubkey::new_unique(),
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        Account {
            address: payer,
            lamports: 1,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        signer_account(new_account),
    ]);
    assert!(result.is_err(), "insufficient funds");
}

// ============================================================================
// transfer
// ============================================================================

#[test]
fn transfer_success() {
    let mut svm = svm_misc();
    let from = Pubkey::new_unique();
    let to = Pubkey::new_unique();

    let ix: Instruction = TransferTestInstruction {
        from,
        to,
        system_program: quasar_svm::system_program::ID,
        amount: 500_000,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(from),
        signer_account(to),
    ]);
    assert!(result.is_ok(), "transfer: {:?}", result.raw_result);
}

#[test]
fn transfer_zero() {
    let mut svm = svm_misc();
    let from = Pubkey::new_unique();
    let to = Pubkey::new_unique();

    let ix: Instruction = TransferTestInstruction {
        from,
        to,
        system_program: quasar_svm::system_program::ID,
        amount: 0,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(from),
        signer_account(to),
    ]);
    assert!(result.is_ok(), "transfer zero: {:?}", result.raw_result);
}

#[test]
fn transfer_full_balance() {
    let mut svm = svm_misc();
    let from = Pubkey::new_unique();
    let to = Pubkey::new_unique();
    let balance = 5_000_000u64;

    let ix: Instruction = TransferTestInstruction {
        from,
        to,
        system_program: quasar_svm::system_program::ID,
        amount: balance,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        Account {
            address: from,
            lamports: balance,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        },
        signer_account(to),
    ]);
    assert!(result.is_ok(), "full balance: {:?}", result.raw_result);

    let from_acc = result.account(&from).expect("from");
    assert_eq!(from_acc.lamports, 0, "drained");
}

#[test]
fn transfer_to_self_borrow_fail() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = TransferTestInstruction {
        from: account,
        to: account,
        system_program: quasar_svm::system_program::ID,
        amount: 100,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        rich_signer_account(account),
    ]);
    assert!(result.is_err(), "self-transfer should fail (borrow conflict)");
}

// ============================================================================
// assign
// ============================================================================

#[test]
fn assign_success() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();
    let new_owner = Pubkey::new_unique();

    let ix: Instruction = AssignTestInstruction {
        account,
        system_program: quasar_svm::system_program::ID,
        owner: new_owner,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        signer_account(account),
    ]);
    assert!(result.is_ok(), "assign: {:?}", result.raw_result);

    let acc = result.account(&account).expect("account");
    assert_eq!(acc.owner, new_owner, "new owner");
}

#[test]
fn assign_to_system_program() {
    let mut svm = svm_misc();
    let account = Pubkey::new_unique();

    let ix: Instruction = AssignTestInstruction {
        account,
        system_program: quasar_svm::system_program::ID,
        owner: quasar_svm::system_program::ID,
    }
    .into();

    let result = svm.process_instruction(&ix, &[
        signer_account(account),
    ]);
    assert!(result.is_ok(), "assign to system: {:?}", result.raw_result);
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test cpi_system:: -- --nocapture 2>&1 | tail -15
git add tests/suite/src/cpi_system.rs && git commit -m "test(cpi): 10 tests for create/transfer/assign"
```

---

## Task 11: errors.rs (11 tests)

**Files:**
- Create: `tests/suite/src/errors.rs`

- [ ] **Step 1: Write errors.rs**

```rust
use {
    crate::helpers::*,
    quasar_svm::{Instruction, Pubkey, ProgramError},
    quasar_test_errors::cpi::*,
};

#[test]
fn custom_error_code() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = CustomErrorInstruction { signer }.into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    result.assert_error(ProgramError::Custom(0)); // TestError::Hello
}

#[test]
fn explicit_error_number() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = ExplicitErrorInstruction { signer }.into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    result.assert_error(ProgramError::Custom(100)); // TestError::ExplicitNum
}

#[test]
fn require_false() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = RequireFalseInstruction { signer }.into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    result.assert_error(ProgramError::Custom(101)); // TestError::RequireFailed
}

#[test]
fn program_error_propagation() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = ProgramErrorInstruction { signer }.into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    result.assert_error(ProgramError::InvalidAccountData);
}

#[test]
fn require_eq_passes() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = RequireEqCheckInstruction {
        signer,
        a: 5,
        b: 5,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_ok(), "eq passes: {:?}", result.raw_result);
}

#[test]
fn require_eq_fails() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = RequireEqCheckInstruction {
        signer,
        a: 1,
        b: 2,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    result.assert_error(ProgramError::Custom(102)); // TestError::RequireEqFailed
}

#[test]
fn require_neq_passes() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = RequireNeqCheckInstruction {
        signer,
        a: 1,
        b: 2,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_ok(), "neq passes: {:?}", result.raw_result);
}

#[test]
fn require_neq_fails() {
    let mut svm = svm_errors();
    let signer = Pubkey::new_unique();

    let ix: Instruction = RequireNeqCheckInstruction {
        signer,
        a: 5,
        b: 5,
    }
    .into();
    let result = svm.process_instruction(&ix, &[signer_account(signer)]);
    assert!(result.is_err());
    // require_neq uses the same error as require_eq in the test program
    result.assert_error(ProgramError::Custom(102));
}

// ============================================================================
// Default framework error codes (no custom error annotation)
// Tests the separate codegen path for default vs custom errors.
// If the framework error mapping regresses, custom-error tests pass but these fail.
// ============================================================================

#[test]
fn has_one_default_mismatch() {
    // has_one = authority (no @ custom error) → default HasOneMismatch (3005)
    let mut svm = svm_errors();
    let authority = Pubkey::new_unique();
    let wrong_authority = Pubkey::new_unique();
    let account = Pubkey::new_unique();

    let ix: Instruction = HasOneDefaultInstruction { authority, account }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(authority),
        error_test_account(account, wrong_authority, 42),
    ]);
    assert!(result.is_err(), "has_one default mismatch");
    result.assert_error(ProgramError::Custom(3005)); // HasOneMismatch
}

#[test]
fn address_default_mismatch() {
    // address = EXPECTED_ADDR_DEFAULT (no @ custom error) → default AddressMismatch (3012)
    let mut svm = svm_errors();
    let wrong = Pubkey::new_unique();

    let ix: Instruction = AddressDefaultInstruction { target: wrong }.into();
    let result = svm.process_instruction(&ix, &[
        error_test_account(wrong, Pubkey::new_unique(), 42),
    ]);
    assert!(result.is_err(), "address default mismatch");
    result.assert_error(ProgramError::Custom(3012)); // AddressMismatch
}

#[test]
fn constraint_default_fail() {
    // constraint = false (no @ custom error) → default ConstraintViolation (3004)
    let mut svm = svm_errors();
    let target = Pubkey::new_unique();

    let ix: Instruction = ConstraintDefaultInstruction { target }.into();
    let result = svm.process_instruction(&ix, &[
        signer_account(target),
    ]);
    assert!(result.is_err(), "constraint default fail");
    result.assert_error(ProgramError::Custom(3004)); // ConstraintViolation
}
```

- [ ] **Step 2: Run & commit**

```bash
cd tests/suite && cargo test errors:: -- --nocapture 2>&1 | tail -15
git add tests/suite/src/errors.rs && git commit -m "test(errors): 11 tests for error codes, require macros, default framework errors"
```

---

## Task 12: Cleanup — delete old file and final verification

- [ ] **Step 1: Delete old accounts.rs**

```bash
rm tests/suite/src/accounts.rs
```

- [ ] **Step 2: Run full test suite**

```bash
cd tests/suite && cargo test 2>&1 | tail -20
```

Expected: All tests pass. No compilation errors.

- [ ] **Step 3: Commit cleanup**

```bash
git add -A tests/suite/src/
git commit -m "test: remove old accounts.rs, complete test suite rewrite"
```

---

## Summary

| File | Tests | Concern |
|------|-------|---------|
| init.rs | 14 | Fresh, prefunded(4), after-close, space override, explicit payer + errors |
| init_if_needed.rs | 10 | New/existing/prefunded + wrong owner/disc/size/writable + front-running |
| close.rs | 5 | Lamport transfer, additive balance + authority/signer errors |
| realloc.rs | 7 | Grow/shrink/noop/large/data preservation/zero-fill/disc-corruption |
| discriminator.rs | 12 | Single/multi-byte valid + wrong/zero/partial/reversed/size + NoDiscAccount(3) |
| optional_accounts.rs | 7 | Some/None sentinel + has_one skip/fail + validation-when-present(2) |
| constraints.rs | 19 | has_one(8) + address(4) + constraint(4) + combined(3) |
| account_validation.rs | 20 | Owner(3) + disc(2) + size(3) + dup(2) + happy(2) + SystemAccount(3) + Program(3) + UncheckedAccount(2) |
| account_flags.rs | 12 | Signer(2) + mut(3) + combined(3) + dup-allowed(3) + double-mut(1) |
| cpi_system.rs | 10 | create(4) + transfer(4) + assign(2) |
| errors.rs | 11 | Custom codes + require_eq/neq + propagation + default framework errors(3) |
| **Total** | **127** | |
