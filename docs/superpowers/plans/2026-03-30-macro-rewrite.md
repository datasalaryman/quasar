# Quasar Macro Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify parsing into a single trait-based API, add a `validate()` hook, expose validation primitives, add debug error provenance, and clean up technical debt.

**Architecture:** Replace the per-struct generated `parse_accounts` inherent method with a generic `walk_svm_buffer<N>` function that handles only buffer walking + dup resolution. All per-field validation (signer, writable, owner, discriminator, PDA, constraints) moves into `ParseAccounts::parse`. The inherent method is removed. `dispatch!` calls the generic walk then the trait.

**Tech Stack:** Rust, proc-macro2/syn/quote (derive macros), solana-account-view, quasar-svm (testing)

**Spec:** `docs/superpowers/specs/2026-03-30-macro-transparency-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `lang/src/buffer.rs` | Create | Generic `walk_svm_buffer<N>` function |
| `lang/src/lib.rs` | Modify | Add `pub mod buffer`, promote header constants to public, re-export helpers |
| `lang/src/traits.rs` | Modify | Add `validate(&self)` default method |
| `lang/src/entrypoint.rs` | Modify | Rewrite `dispatch!` to call generic walk + trait |
| `lang/src/checks/mod.rs` | Modify | Add public helper functions (`require_signer`, etc.) |
| `derive/src/accounts/mod.rs` | Modify | Move header checks into `parse_body`, remove inherent method, call `validate()` |
| `derive/src/accounts/fields.rs` | Modify | Extend debug logging to all check types |
| `spl/src/instructions/mod.rs` | Modify | Remove stale lifecycle trait doc comment |
| `derive/src/declare_program.rs` | Modify | Remove dead `defined` field |
| Various derive files | Modify | Replace `unwrap()` with `.expect()` |

---

### Task 1: Cleanup — stale docs, dead code, allow(dead_code) audit

Low-risk housekeeping. No behavioral change. Ship first to reduce noise in later diffs.

**Files:**
- Modify: `spl/src/instructions/mod.rs` (stale lifecycle trait comment)
- Modify: `derive/src/declare_program.rs:52` (dead `defined` field)
- Modify: `idl/src/parser/helpers.rs:139` (allow(dead_code) audit)
- Modify: `spl/src/instructions/initialize_account.rs:26` (allow(dead_code) audit)
- Modify: `spl/src/instructions/initialize_mint.rs:28` (allow(dead_code) audit)
- Modify: `lang/src/lib.rs:79` (allow(dead_code) audit)

- [ ] **Step 1: Remove stale lifecycle trait doc comment**

In `spl/src/instructions/mod.rs`, find the line referencing lifecycle traits (`InitToken`, `InitMint`, `TokenClose`) and either remove the comment or update it to reflect reality.

- [ ] **Step 2: Remove dead `defined` field in `declare_program.rs`**

In `derive/src/declare_program.rs`, the `IdlType::Defined` variant has a `#[allow(dead_code)]` field `defined: String`. Check if the JSON IDL schema requires this field for deserialization. If yes, keep the field but add a comment explaining why. If no, remove the variant or the field.

- [ ] **Step 3: Audit each `#[allow(dead_code)]` location**

For each of these locations, read the code and determine if the dead code should be removed or documented:

- `idl/src/parser/helpers.rs:139`
- `spl/src/instructions/initialize_account.rs:26`
- `spl/src/instructions/initialize_mint.rs:28`
- `lang/src/lib.rs:79` (the `log_str` function — this IS used by generated code under `#[cfg(feature = "debug")]`, so the allow is justified; add a comment explaining why)

- [ ] **Step 4: Verify everything compiles**

Run: `cargo check --workspace`
Expected: No errors, no new warnings.

- [ ] **Step 5: Commit**

```bash
git add spl/src/instructions/mod.rs derive/src/declare_program.rs idl/src/parser/helpers.rs spl/src/instructions/initialize_account.rs spl/src/instructions/initialize_mint.rs lang/src/lib.rs
git commit -m "chore: remove stale docs, dead code, audit allow(dead_code)"
```

---

### Task 2: Cleanup — replace `unwrap()` with `.expect()` in derive macros

**Files:**
- Modify: all files in `derive/src/` containing `unwrap()`

- [ ] **Step 1: Find all unwrap() calls in derive code**

Run: `grep -rn '\.unwrap()' derive/src/ | grep -v test`

For each call, replace `.unwrap()` with `.expect("descriptive message")` that explains what was expected. For example:
- `.unwrap()` on `field.ident` → `.expect("field must have an identifier")`
- `.unwrap()` on `parse_args` → `.expect("failed to parse attribute arguments")`
- `.unwrap()` on `first()` → `.expect("expected at least one field")`

Do NOT change `.unwrap()` in test code.

- [ ] **Step 2: Verify everything compiles**

Run: `cargo check --workspace`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add derive/src/
git commit -m "chore: replace unwrap() with expect() in derive macros"
```

---

### Task 3: Create generic `walk_svm_buffer` function

This is the foundation of the architecture change. Create the generic buffer walk function that replaces the per-struct `parse_accounts` inherent method.

**Files:**
- Create: `lang/src/buffer.rs`
- Modify: `lang/src/lib.rs` (add `pub mod buffer`)

- [ ] **Step 1: Create `lang/src/buffer.rs`**

```rust
//! Generic SVM input buffer walker.
//!
//! `walk_svm_buffer` turns the raw SVM input into an array of `AccountView`s,
//! handling dup resolution. It does NOT perform per-field validation (signer,
//! writable, owner, etc.) — that happens in `ParseAccounts::parse`.

use {
    crate::prelude::ProgramError,
    crate::utils::hint,
    solana_account_view::{AccountView, RuntimeAccount, MAX_PERMITTED_DATA_INCREASE, NOT_BORROWED},
};

/// Size of a `RuntimeAccount` header plus trailing data region and padding.
const ACCOUNT_HEADER: usize =
    core::mem::size_of::<RuntimeAccount>() + MAX_PERMITTED_DATA_INCREASE + core::mem::size_of::<u64>();

/// Walk the SVM input buffer, creating `AccountView`s and resolving duplicates.
///
/// This function is generic over `N` (the account count). `dispatch!` provides
/// `N` via `<T as AccountCount>::COUNT`, so LLVM knows the trip count at compile
/// time and can unroll the loop.
///
/// # Safety
///
/// - `input` must point to the start of the first account entry in the SVM
///   input buffer (past the 8-byte account count).
/// - `buf` must be an uninitialized array of exactly `N` `AccountView` slots.
/// - The SVM buffer must contain at least `N` account entries.
#[inline(always)]
pub unsafe fn walk_svm_buffer<const N: usize>(
    mut input: *mut u8,
    buf: &mut core::mem::MaybeUninit<[AccountView; N]>,
) -> Result<*mut u8, ProgramError> {
    let base = buf.as_mut_ptr() as *mut AccountView;

    let mut i = 0;
    while i < N {
        let raw = input as *mut RuntimeAccount;
        let borrow_state = (*raw).borrow_state;

        if borrow_state == NOT_BORROWED {
            // Fresh account — wrap it as an AccountView.
            core::ptr::write(base.add(i), AccountView::new_unchecked(raw));
            input = input.add(ACCOUNT_HEADER + (*raw).data_len as usize);
            // Align to 8-byte boundary.
            input = input.add((input as usize).wrapping_neg() & 7);
        } else {
            // Duplicate — copy the AccountView from the referenced slot.
            let idx = borrow_state as usize;
            if hint::unlikely(idx >= i) {
                return Err(ProgramError::InvalidAccountData);
            }
            core::ptr::write(base.add(i), core::ptr::read(base.add(idx)));
            input = input.add(core::mem::size_of::<u64>());
        }
        i += 1;
    }
    Ok(input)
}
```

- [ ] **Step 2: Register the module in `lang/src/lib.rs`**

Add `pub mod buffer;` after the existing `pub mod borsh;` line (around line 94). Follow the existing doc comment style:

```rust
/// Generic SVM input buffer walker for `dispatch!`.
pub mod buffer;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p quasar-lang`
Expected: No errors. The function is not called yet — just ensure it compiles.

- [ ] **Step 4: Commit**

```bash
git add lang/src/buffer.rs lang/src/lib.rs
git commit -m "feat: add generic walk_svm_buffer function"
```

---

### Task 4: Add `validate(&self)` to `ParseAccounts` trait

**Files:**
- Modify: `lang/src/traits.rs:125-152`

- [ ] **Step 1: Add the `validate` method to the trait**

In `lang/src/traits.rs`, add `validate` between `parse_with_instruction_data` and `epilogue`:

```rust
    /// User-defined validation hook called after all field-level checks pass
    /// but before the instruction handler executes.
    ///
    /// Override this to add cross-field validation that the `#[account(...)]`
    /// attribute DSL cannot express. The default implementation is a no-op.
    ///
    /// Lifecycle: `parse()` -> `validate()` -> handler -> `epilogue()`
    ///
    /// The signature is `&self` (not `&mut self`) — validation must not mutate
    /// validated account references.
    #[inline(always)]
    fn validate(&self) -> Result<(), ProgramError> {
        Ok(())
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p quasar-lang`
Expected: No errors. Existing code is unaffected (default no-op).

- [ ] **Step 3: Commit**

```bash
git add lang/src/traits.rs
git commit -m "feat: add validate() hook to ParseAccounts trait"
```

---

### Task 5: Expose validation helpers as public API

**Files:**
- Modify: `lang/src/checks/mod.rs` (or create `lang/src/checks/helpers.rs`)
- Modify: `lang/src/lib.rs` (update `__internal` exports and prelude)

- [ ] **Step 1: Check current checks module structure**

Read `lang/src/checks/mod.rs` to understand existing layout.

- [ ] **Step 2: Add public helper functions**

Add a new file `lang/src/checks/helpers.rs` with standalone validation helpers for manual `ParseAccounts` implementers:

```rust
//! Standalone validation helpers for manual `ParseAccounts` implementations.
//!
//! These functions wrap the same checks that `#[derive(Accounts)]` generates,
//! exposed as composable building blocks.

use {
    crate::prelude::{AccountView, Address, ProgramError},
    crate::utils::hint::unlikely,
};

/// Verify that the account is a transaction signer.
#[inline(always)]
pub fn require_signer(view: &AccountView) -> Result<(), ProgramError> {
    if unlikely(!view.is_signer()) {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

/// Verify that the account is writable.
#[inline(always)]
pub fn require_writable(view: &AccountView) -> Result<(), ProgramError> {
    if unlikely(!view.is_writable()) {
        return Err(ProgramError::Immutable);
    }
    Ok(())
}

/// Verify that the account is executable.
#[inline(always)]
pub fn require_executable(view: &AccountView) -> Result<(), ProgramError> {
    if unlikely(!view.executable()) {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

/// Verify that the account address matches an expected value.
#[inline(always)]
pub fn require_address(view: &AccountView, expected: &Address) -> Result<(), ProgramError> {
    if unlikely(!crate::keys_eq(view.address(), expected)) {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}
```

- [ ] **Step 3: Register in checks module and re-export**

In `lang/src/checks/mod.rs`, add:
```rust
pub mod helpers;
```

In `lang/src/prelude.rs` (or wherever the prelude is defined), re-export the helpers so users can access them as `quasar_lang::prelude::require_signer` etc.

- [ ] **Step 4: Promote header constants from `__internal` to public**

In `lang/src/lib.rs`, add a public constants section (or add to `buffer.rs`):

```rust
/// Header validation constants for manual `ParseAccounts` implementations.
///
/// These are the expected u32 values for the first 4 bytes of a `RuntimeAccount`.
/// Use with `AccountView::account_ptr()` to perform fused flag checks.
pub mod header {
    pub use crate::__internal::{
        NODUP, NODUP_EXECUTABLE, NODUP_MUT, NODUP_MUT_SIGNER, NODUP_SIGNER,
    };

    /// Mask for flag bits (signer + writable + executable), excluding borrow_state.
    pub const FLAG_MASK: u32 = 0xFFFFFF00 | 0xFF;
}
```

Note: Check what `FLAG_MASK` value is used in the derive code (`derive/src/accounts/field_kind.rs`) and use the same value.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p quasar-lang`
Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add lang/src/checks/ lang/src/lib.rs lang/src/prelude.rs
git commit -m "feat: expose validation helpers and header constants as public API"
```

---

### Task 6: Rewrite `dispatch!` to use generic buffer walk + trait

This is the core architecture change. `dispatch!` stops calling the per-struct inherent method and instead calls `walk_svm_buffer` + `ParseAccounts::parse`.

**Files:**
- Modify: `lang/src/entrypoint.rs:12-68`

- [ ] **Step 1: Rewrite the `dispatch!` macro**

Replace the match arm body in `lang/src/entrypoint.rs`. The key change: instead of calling `<$accounts_ty>::parse_accounts(...)`, call `quasar_lang::buffer::walk_svm_buffer` then route through `ParseAccounts`.

```rust
#[macro_export]
macro_rules! dispatch {
    ($ptr:expr, $ix_data:expr, $disc_len:literal, {
        $([$($disc_byte:literal),+] => $handler:ident($accounts_ty:ty)),+ $(,)?
    }) => {{
        let __program_id: &[u8; 32] = unsafe {
            &*($ix_data.as_ptr().add($ix_data.len()) as *const [u8; 32])
        };
        const __U64_SIZE: usize = core::mem::size_of::<u64>();
        let __num_accounts = unsafe { *($ptr as *const u64) };
        let __accounts_start = unsafe { ($ptr as *mut u8).add(__U64_SIZE) };

        if $ix_data.len() < $disc_len {
            return Err(ProgramError::InvalidInstructionData);
        }
        let __disc: [u8; $disc_len] = unsafe {
            *($ix_data.as_ptr() as *const [u8; $disc_len])
        };
        match __disc {
            $(
                [$($disc_byte),+] => {
                    if (__num_accounts as usize) < <$accounts_ty as AccountCount>::COUNT {
                        return Err(ProgramError::NotEnoughAccountKeys);
                    }
                    let mut __buf = core::mem::MaybeUninit::<
                        [quasar_lang::__internal::AccountView; <$accounts_ty as AccountCount>::COUNT]
                    >::uninit();
                    let __remaining_ptr = unsafe {
                        $crate::buffer::walk_svm_buffer::<{ <$accounts_ty as AccountCount>::COUNT }>(
                            __accounts_start,
                            &mut __buf,
                        )?
                    };
                    let mut __accounts = unsafe { __buf.assume_init() };
                    $handler(Context {
                        program_id: __program_id,
                        accounts: &mut __accounts,
                        remaining_ptr: __remaining_ptr,
                        data: $ix_data,
                        accounts_boundary: unsafe { $ix_data.as_ptr().sub(__U64_SIZE) },
                    })
                }
            ),+
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }};
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: No errors. The dispatch macro now calls `walk_svm_buffer` instead of the inherent method, but the inherent method still exists (not yet removed). Both paths should work.

- [ ] **Step 3: Commit**

```bash
git add lang/src/entrypoint.rs
git commit -m "feat: dispatch! uses generic walk_svm_buffer instead of inherent method"
```

---

### Task 7: Move header validation from inherent method into `ParseAccounts::parse`

This is the biggest derive macro change. The per-field header flag checks (signer, writable, executable, dup) move from the generated `parse_accounts` inherent method into the generated `ParseAccounts::parse` trait impl.

**Files:**
- Modify: `derive/src/accounts/mod.rs` (major rewrite of codegen)
- Modify: `derive/src/accounts/fields.rs` (header check generation for parse body)

- [ ] **Step 1: Understand the current split**

Read `derive/src/accounts/mod.rs` carefully. Currently:
- Lines 114-292: `parse_steps` — generated for the inherent `parse_accounts`, does header validation per field
- Lines 294+: `parse_body` — generated for `ParseAccounts::parse`, does owner/discriminator/PDA/constraint checks
- Lines 628-646: the inherent `parse_accounts` method that uses `parse_steps`

The goal: merge `parse_steps` logic INTO `parse_body`. The inherent method goes away.

- [ ] **Step 2: Move header flag checks into the parse body**

In the generated `ParseAccounts::parse`, add header validation at the start of each field's check block. The fused u32 check reads through `AccountView::account_ptr()`:

For non-dup, non-optional fields, generate:
```rust
{
    let __header = unsafe { *(accounts[#idx].account_ptr() as *const u32) };
    if quasar_lang::utils::hint::unlikely(__header != quasar_lang::__internal::#nodup_const_ident) {
        #[cfg(feature = "debug")]
        quasar_lang::__internal::log_str(concat!(
            "Account '", stringify!(#field_name),
            "' (index ", #account_index, "): ", #debug_msg
        ));
        return Err(ProgramError::from(quasar_lang::decode_header_error(__header, quasar_lang::__internal::#nodup_const_ident)));
    }
}
```

For dup-aware fields, generate:
```rust
{
    let __header = unsafe { *(accounts[#idx].account_ptr() as *const u32) };
    if quasar_lang::utils::hint::unlikely((__header & #flag_mask) != #expected_masked) {
        #[cfg(feature = "debug")]
        quasar_lang::__internal::log_str(concat!(
            "Account '", stringify!(#field_name),
            "' (index ", #account_index, "): header flags mismatch"
        ));
        return Err(ProgramError::from(quasar_lang::decode_header_error(__header, #expected_header)));
    }
}
```

For optional fields, wrap with sentinel guard:
```rust
if !quasar_lang::keys_eq(accounts[#idx].address(), __program_id) {
    // header flag check
}
```

- [ ] **Step 3: Add `validate()` call to the generated parse**

After all field checks and struct construction, before returning:

```rust
let __result = Self { #(#field_names),* };
__result.validate()?;
Ok((__result, #bumps_name { #(#bump_fields),* }))
```

- [ ] **Step 4: Remove the inherent `parse_accounts` method generation**

In `derive/src/accounts/mod.rs`, remove lines 628-646 (the `impl<'info> #name<'info> { pub unsafe fn parse_accounts(...) }` block) and the `parse_steps` vector that feeds it. The `expanded` quote block should no longer include the inherent method.

- [ ] **Step 5: Remove `parse_steps` generation**

The entire `parse_steps` vector (lines 116-292) is no longer needed. Remove it. The header check logic now lives in the parse body alongside the owner/discriminator/PDA checks.

- [ ] **Step 6: Update composite type handling**

For composite types, the current code calls `<inner_ty>::parse_accounts(...)` (the inherent method). Change this to use `walk_svm_buffer` + `ParseAccounts::parse` for the inner type, OR simply call `<inner_ty as ParseAccounts>::parse(...)` on the relevant slice of the already-walked buffer. The buffer was walked by `dispatch!`; the composite just reads from `accounts[offset..offset+inner_count]`.

The composite path in `parse_body` should generate:
```rust
let (#child_name, #child_bumps) = <#inner_ty as ParseAccounts>::parse(
    &mut accounts[#start..#end],
    __program_id,
)?;
```

Since the buffer walk already happened in `dispatch!`, the composite just indexes into the pre-walked `accounts` slice.

- [ ] **Step 7: Verify it compiles**

Run: `cargo check --workspace`
Expected: No errors.

- [ ] **Step 8: Build test programs and run the test suite**

```bash
# Build all test programs for SBF
cargo build-sbf --manifest-path tests/programs/test-misc/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-pda/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-errors/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-events/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-sysvar/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-init/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-validate/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-cpi/Cargo.toml

# Run the full test suite
cargo test -p quasar-test-suite
```

Expected: ALL tests pass. If any fail, the header check codegen is wrong — debug by comparing the generated code (via `cargo expand`) with the old output.

Also run the example program tests:
```bash
cargo build-sbf --manifest-path examples/vault/Cargo.toml
cargo test -p quasar-vault

cargo build-sbf --manifest-path examples/escrow/Cargo.toml
cargo test -p quasar-escrow
```

- [ ] **Step 9: Run Miri tests**

```bash
cargo +nightly miri test -p quasar-lang
cargo +nightly miri test -p quasar-spl
```

Expected: No UB detected.

- [ ] **Step 10: Commit**

```bash
git add derive/src/accounts/
git commit -m "feat: move all validation into ParseAccounts::parse, remove inherent parse_accounts"
```

---

### Task 8: Extend debug error provenance to all check types

Currently debug logging exists for header mismatches only. Extend to owner, discriminator, PDA, and constraint failures.

**Files:**
- Modify: `derive/src/accounts/fields.rs`

- [ ] **Step 1: Read current debug logging in fields.rs**

Find all places where `ProgramError` is returned during field validation. These include:
- Owner checks (`CheckOwner::check_owner`)
- Discriminator checks (`AccountCheck::check`)
- PDA validation (`verify_program_address` / `based_try_find_program_address`)
- Constraint evaluation
- `has_one` checks
- Address checks

- [ ] **Step 2: Add debug logging before each error return**

For each error path, add a `#[cfg(feature = "debug")]` log message with the field name, index, and check type. Pattern:

```rust
#[cfg(feature = "debug")]
quasar_lang::__internal::log_str(concat!(
    "Account '", stringify!(#field_name),
    "' (index ", #account_index, "): owner mismatch"
));
```

Do this for:
- Owner check failures: `"owner mismatch"`
- Discriminator failures: `"discriminator mismatch"`
- PDA failures: `"PDA verification failed"`
- Constraint failures: `"constraint failed: <expr>"`
- has_one failures: `"has_one check failed: <field>"`
- Address failures: `"address mismatch"`

- [ ] **Step 3: Build test-errors with debug feature and verify**

```bash
cargo build-sbf --manifest-path tests/programs/test-errors/Cargo.toml --features debug,alloc
cargo test -p quasar-test-suite --features debug -- test_header --nocapture
```

Expected: Tests pass and debug messages appear in output.

- [ ] **Step 4: Commit**

```bash
git add derive/src/accounts/fields.rs
git commit -m "feat: add debug error provenance for all validation check types"
```

---

### Task 9: Final verification — full test suite + examples

End-to-end verification that all changes work together.

**Files:** None (testing only)

- [ ] **Step 1: Clean build all test programs**

```bash
cargo build-sbf --manifest-path tests/programs/test-misc/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-pda/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-errors/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-events/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-sysvar/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-init/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-validate/Cargo.toml
cargo build-sbf --manifest-path tests/programs/test-token-cpi/Cargo.toml
```

- [ ] **Step 2: Run full test suite**

```bash
cargo test -p quasar-test-suite
```

Expected: ALL tests pass.

- [ ] **Step 3: Build and test examples**

```bash
cargo build-sbf --manifest-path examples/vault/Cargo.toml
cargo test -p quasar-vault

cargo build-sbf --manifest-path examples/escrow/Cargo.toml
cargo test -p quasar-escrow

cargo build-sbf --manifest-path examples/multisig/Cargo.toml
cargo test -p quasar-multisig
```

Expected: ALL tests pass.

- [ ] **Step 4: Run Miri**

```bash
cargo +nightly miri test -p quasar-lang
cargo +nightly miri test -p quasar-spl
```

Expected: No UB.

- [ ] **Step 5: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: No warnings.

- [ ] **Step 6: Verify .so sizes haven't regressed**

```bash
ls -la target/deploy/*.so | awk '{print $5, $9}'
```

Compare against pre-rewrite sizes. The generic buffer walk should produce equal or smaller binaries since the per-struct buffer walk code is eliminated.
