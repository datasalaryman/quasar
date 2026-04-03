# Quasar Macro Rewrite: Transparency, Ejectability & Cleanup

Internal spec — concrete changes to make `ParseAccounts` the sole parsing API, improve developer experience, and clean up technical debt.

## Philosophy

**"Macros are sugar, traits are the API."** Every macro maps to traits you can implement yourself. The framework provides one generic buffer walk. All per-field validation lives in `ParseAccounts::parse`.

---

## 1. Unify parsing: generic buffer walk + trait-only validation

### Problem

Today there are two parsing layers: an inherent `parse_accounts` method (generated per-struct, does raw pointer buffer walking + header flag validation) and `ParseAccounts::parse` (trait, does typed construction + owner/discriminator/PDA/constraint checks). `dispatch!` calls the inherent method. A user who manually implements `ParseAccounts` can't plug into `dispatch!` because it never calls the trait.

### Solution

**A. Generic `walk_svm_buffer` function** (new, ~30 lines in `quasar_lang::buffer`)

Handles ONLY buffer walking + dup resolution. No per-field validation. Identical for every struct.

```rust
#[inline(always)]
pub unsafe fn walk_svm_buffer<const N: usize>(
    mut input: *mut u8,
    buf: &mut MaybeUninit<[AccountView; N]>,
) -> Result<*mut u8, ProgramError> {
    let base = buf.as_mut_ptr() as *mut AccountView;
    let mut i = 0;
    while i < N {
        let raw = input as *mut RuntimeAccount;
        let borrow_state = (*raw).borrow_state;
        if borrow_state == NOT_BORROWED {
            core::ptr::write(base.add(i), AccountView::new_unchecked(raw));
            input = input.add(ACCOUNT_HEADER + (*raw).data_len as usize);
            input = input.add((input as usize).wrapping_neg() & 7);
        } else {
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

**B. `dispatch!` calls the generic walk then `ParseAccounts::parse`**

```rust
let __remaining_ptr = unsafe {
    quasar_lang::walk_svm_buffer::<{ <$accounts_ty as AccountCount>::COUNT }>(
        __accounts_start, &mut __buf,
    )?
};
let mut __accounts = unsafe { __buf.assume_init() };
// The trait is the only parsing API
let (parsed, bumps) = <$accounts_ty as ParseAccounts>::parse(&mut __accounts, __program_id)?;
```

**C. ALL per-field validation moves to `ParseAccounts::parse`**

The derive-generated `parse()` does the fused u32 header check via `AccountView::account_ptr()` (public on AccountView), then owner/discriminator/PDA/constraint checks. The same optimization, just in the trait method instead of the inherent method.

```rust
// Fused check still works — reads header through AccountView's raw pointer
let header = unsafe { *(accounts[0].account_ptr() as *const u32) };
if unlikely((header & FLAG_MASK) != EXPECTED_MUT_SIGNER) {
    return Err(decode_header_error(header, EXPECTED_MUT_SIGNER));
}
```

**D. The inherent `parse_accounts` method is removed entirely.**

### CU impact

CU-neutral on happy path (same u32 comparison, same inlined code). Potentially CU-negative on error path (buffer walk is cheaper without flag checks, fail-fast happens in parse).

### Files affected

- `lang/src/buffer.rs` — new module, `walk_svm_buffer`
- `lang/src/entrypoint.rs` — `dispatch!` macro rewrite
- `derive/src/accounts/mod.rs` — move header checks from `parse_steps` into `parse_body`, remove inherent method generation (lines 628-646)
- `derive/src/accounts/fields.rs` — header check codegen moves into parse body
- `lang/src/traits.rs` — no signature change, but document the new contract

---

## 2. Add `validate(&self)` hook on `ParseAccounts`

### What

Add a default no-op method on the `ParseAccounts` trait, called after all field-level validation in the generated `parse()`. Follows the existing `epilogue()` pattern.

```rust
// In traits.rs
pub trait ParseAccounts<'info>: Sized {
    type Bumps: Copy;
    fn parse(...) -> Result<(Self, Self::Bumps), ProgramError>;
    fn parse_with_instruction_data(...) -> Result<(Self, Self::Bumps), ProgramError> { ... }

    #[inline(always)]
    fn validate(&self) -> Result<(), ProgramError> { Ok(()) }

    #[inline(always)]
    fn epilogue(&mut self) -> Result<(), ProgramError> { Ok(()) }
}
```

Signature is `&self` not `&mut self` — validation must not mutate validated references (security review finding).

### Lifecycle

`parse()` (field validation) -> `validate()` (user custom checks) -> instruction handler -> `epilogue()` (close/sweep cleanup)

### Files affected

- `lang/src/traits.rs` — add `validate` method
- `derive/src/accounts/mod.rs` — generated `parse()` calls `result.validate()?` before returning

---

## 3. Expose validation primitives as public API

### What

Make the building blocks available so manual `ParseAccounts` implementers compose from battle-tested functions instead of raw pointer code.

**Header constants** (already in `__internal`, promote to documented public API):
- `NODUP`, `NODUP_MUT`, `NODUP_SIGNER`, `NODUP_MUT_SIGNER`, `NODUP_EXECUTABLE`, `FLAG_MASK`

**Helper functions** (new, in `quasar_lang::checks` or similar):
- `require_signer(view: &AccountView) -> Result<(), ProgramError>`
- `require_writable(view: &AccountView) -> Result<(), ProgramError>`
- `require_executable(view: &AccountView) -> Result<(), ProgramError>`
- `check_owner<T: Owner>(view: &AccountView) -> Result<(), ProgramError>`
- `check_discriminator<T: Discriminator>(view: &AccountView) -> Result<(), ProgramError>`
- `verify_pda(seeds: &[&[u8]], address: &Address, program_id: &Address) -> Result<u8, ProgramError>`

**SPL validation** (already exists in `quasar_spl`, ensure public):
- `validate_token_account`, `validate_ata`, `validate_mint`

### Files affected

- `lang/src/lib.rs` — re-export header constants
- `lang/src/checks/` — new helper functions (or add to existing check modules)
- `spl/src/validate.rs` — verify pub visibility

---

## 4. Add error provenance under `debug` feature flag

### What

When `parse()` fails, log which field and which check failed. Gate behind `#[cfg(feature = "debug")]` to avoid CU cost in production.

The derive already has partial support — the current code emits `log_str` calls under `#[cfg(feature = "debug")]` for header mismatches (see `derive/src/accounts/mod.rs` lines 191-194, 272-276). Extend this to ALL check types: owner checks, discriminator checks, PDA validation, constraint failures.

```
"Account 'vault' (index 1): owner mismatch — expected <program_id>, got <actual>"
"Account 'escrow' (index 2): PDA check failed — seeds [b'escrow', maker]"
"Account 'maker_ta_b' (index 5): constraint failed"
```

### Files affected

- `derive/src/accounts/fields.rs` — add debug logging to owner checks, discriminator checks, PDA validation, constraint evaluation
- `derive/src/accounts/mod.rs` — ensure all check paths have debug annotations

---

## 5. Document `realloc`/`realloc::payer` as core attributes

### What

The core attribute set is 14 (not 13 — `signer` is type-level, but `realloc`/`realloc::payer` are core):

`mut`, `init`, `init_if_needed`, `seeds`, `bump`, `payer`, `space`, `has_one`, `constraint`, `address`, `close`, `dup`, `realloc`, `realloc::payer`

`sweep` is SPL-specific (token/ATA only).

### Files affected

- Documentation only

---

## 6. Cleanup: stale doc comment

Remove lifecycle trait reference in `spl/src/instructions/mod.rs` that mentions `InitToken`/`InitMint` as traits — these never existed as traits.

---

## 7. Cleanup: dead code in `declare_program.rs`

Remove dead `defined: String` field and its `#[allow(dead_code)]` at `derive/src/declare_program.rs:52`, or use it if IDL defined types should be supported.

---

## 8. Cleanup: audit `#[allow(dead_code)]` attributes

Review each and either remove the dead code or document why it's kept:
- `idl/src/parser/helpers.rs:139`
- `spl/src/instructions/initialize_account.rs:26`
- `spl/src/instructions/initialize_mint.rs:28`
- `lang/src/lib.rs:79`

---

## 9. Cleanup: replace `unwrap()` with `.expect()` in derive macros

52 `unwrap()` calls in derive code. Safe at compile-time but useless error messages when they fire during macro expansion. Replace with `.expect("descriptive message")`.

---

## Implementation order

1. **Items 6-9 (cleanup)** — low risk, no behavioral change, can be done first
2. **Item 1 (generic buffer walk + trait unification)** — the big architectural change, do this before adding new trait methods
3. **Item 2 (validate hook)** — depends on item 1 (adds to the trait that item 1 restructures)
4. **Item 3 (expose validation primitives)** — depends on item 1 (the helpers need to match the new parsing model)
5. **Item 4 (error provenance)** — depends on item 1 (debug logging needs to match the new codegen)
6. **Item 5 (documentation)** — after all code changes settle
