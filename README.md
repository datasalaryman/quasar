<h1 align="center">
  <code>quasar</code>
</h1>
<p align="center">
  Write optimized Solana programs without thinking too much.
</p>

## Overview

Quasar is a `no_std` Solana program framework that brings everything the ecosystem has learned about CU optimization — from [Pinocchio](https://github.com/anza-xyz/pinocchio/blob/main/README.md) programs to zero-copy tricks — into a declarative macro system with Anchor-level developer experience. 

It provides `#[account]`, `#[derive(Accounts)]`, `#[instruction]`, `#[program]`, `#[event]` — but the generated code is zero-copy and zero-allocation, operating directly on the SVM input buffer with no deserialization step.

The framework is a workspace of six crates:

| Crate | Path | Purpose |
|-------|------|---------|
| `quasar` | `quasar/` | Facade crate — the single dependency for programs |
| `quasar-core` | `core/` | Account types, CPI builder, events, sysvars, error handling |
| `quasar-derive` | `derive/` | Proc macros for accounts, instructions, programs, events, errors |
| `quasar-pod` | `pod/` | Alignment-1 integer types — usable independently of the framework |
| `quasar-spl` | `spl/` | SPL Token program CPI and zero-copy `TokenAccountState` |
| `quasar-idl` | `idl/` | IDL generator with discriminator collision detection |

Add to your program's `Cargo.toml`:

```toml
[dependencies]
quasar = "0.1"
```

This re-exports `quasar-core` and `quasar-spl` (via the `spl` feature, on by default).

## Writing a Program

A complete escrow program. Three instructions, one account type, typed CPI, PDA validation, events — all in ~80 lines:

### State

```rust
use quasar_core::prelude::*;

#[account(discriminator = 1)]
pub struct EscrowAccount {
    pub maker: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub maker_ta_b: Address,
    pub receive: u64,
    pub bump: u8,
}
```

`#[account]` generates a zero-copy companion struct where `u64` becomes `PodU64` (alignment 1), a `Deref` impl for direct field access, discriminator/space/owner trait impls, and `init()` with re-initialization protection. A compile-time assertion rejects all-zero discriminators — they're indistinguishable from uninitialized account data.

### Accounts

```rust
#[derive(Accounts)]
pub struct Make<'info> {
    pub maker: &'info mut Signer,
    #[account(seeds = [b"escrow", maker], bump)]
    pub escrow: &'info mut Initialize<EscrowAccount>,
    pub maker_ta_a: &'info mut Account<TokenAccount>,
    pub maker_ta_b: &'info Account<TokenAccount>,
    pub vault_ta_a: &'info mut Account<TokenAccount>,
    pub rent: &'info Rent,
    pub token_program: &'info TokenProgram,
    pub system_program: &'info SystemProgram,
}
```

Account directives:

- **`mut`** — asserts the account is writable
- **`has_one = field`** — cross-account validation (e.g., `escrow.maker == maker.address()`)
- **`constraint = expr`** — arbitrary boolean check
- **`seeds = [...], bump`** — PDA derivation via `find_program_address`
- **`seeds = [...], bump = expr`** — PDA verification via `create_program_address` (cheaper when bump is known)

`&'info mut` references automatically generate writable checks. `Signer` checks the `is_signer` flag. `Account<T>` validates owner and discriminator. `Initialize<T>` skips validation for accounts that don't exist yet.

### Instructions

```rust
#[program]
mod quasar_escrow {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn make(ctx: Ctx<Make>, deposit: u64, receive: u64) -> Result<(), ProgramError> {
        ctx.accounts.make_escrow(receive, &ctx.bumps)?;
        ctx.accounts.emit_event(deposit, receive)?;
        ctx.accounts.deposit_tokens(deposit)
    }

    #[instruction(discriminator = 1)]
    pub fn take(ctx: Ctx<Take>) -> Result<(), ProgramError> {
        ctx.accounts.transfer_tokens()?;
        ctx.accounts.withdraw_tokens_and_close(&ctx.bumps)?;
        ctx.accounts.emit_event()?;
        ctx.accounts.close_escrow()
    }

    #[instruction(discriminator = 2)]
    pub fn refund(ctx: Ctx<Refund>) -> Result<(), ProgramError> {
        ctx.accounts.withdraw_tokens_and_close(&ctx.bumps)?;
        ctx.accounts.emit_event()?;
        ctx.accounts.close_escrow()
    }
}
```

Discriminators are explicit integers — no sha256 hashing. The `#[program]` macro generates the entrypoint, dispatch logic, self-CPI event handler, and an off-chain client module with instruction builder structs. Discriminator collisions and `0xFF` conflicts (reserved for events) are caught at compile time.

Instruction arguments (`deposit`, `receive`) are deserialized through a generated zero-copy struct with a compile-time alignment assertion — same pattern as account state, no allocation.

### CPI

```rust
// SPL Token transfer
self.token_program.transfer(
    self.maker_ta_a,
    self.vault_ta_a,
    self.maker,
    amount,
).invoke()?;

// PDA-signed system program call
let seeds = bumps.escrow_seeds();
system_program.create_account(payer, escrow, lamports, space, &owner)
    .invoke_signed(&seeds)?;
```

CPI uses `CpiCall<'a, const ACCTS: usize, const DATA: usize>` — account count and data size are const generics, so everything lives on the stack. `invoke()` calls `sol_invoke_signed_c` directly with a pre-built `RawCpiAccount` array (56 bytes per account, layout verified at compile time). No heap allocation, no intermediate instruction view.

The bumps struct captures account addresses at parse time and exposes `*_seeds()` methods that return fixed-size `[Seed; N]` arrays — PDA seeds are reconstructed without re-derivation.

### Events

```rust
#[event(discriminator = 0)]
pub struct MakeEvent {
    pub escrow: Address,
    pub maker: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub deposit: u64,
    pub receive: u64,
}
```

Two emission paths:

- **`emit!(event)`** — `sol_log_data` syscall, ~100 CU. Spoofable by any program.
- **`program.emit_event(&event, &event_authority)`** — self-CPI with `0xFF`-prefixed instruction data. The callee validates the event authority PDA. ~1,000 CU. Not spoofable.

Event serialization is `memcpy` from the `#[repr(C)]` struct. A compile-time assertion guarantees no padding exists — if a field introduces padding, the build fails.

## What Gets Generated

Understanding what macros produce is critical for trusting a framework. Here's what `#[account(discriminator = 1)]` generates for `EscrowAccount`:

**Zero-copy companion struct** — `u64` → `PodU64`, `u8` → stays `u8`. Alignment 1 enforced at compile time:
```rust
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EscrowAccountZc {
    pub maker: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub maker_ta_b: Address,
    pub receive: PodU64,
    pub bump: u8,
}

const _: () = assert!(align_of::<EscrowAccountZc>() == 1);
```

**Discriminator validation** — byte-level check, no slice comparison:
```rust
impl AccountCheck for EscrowAccount {
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        let data = unsafe { view.borrow_unchecked() };
        if data.len() < 1 + size_of::<EscrowAccountZc>() {
            return Err(ProgramError::AccountDataTooSmall);
        }
        if unsafe { *data.get_unchecked(0) } != 1 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}
```

**Zero-copy `Deref`** — `Account<EscrowAccount>` dereferences to `&EscrowAccountZc`. No deserialization, no allocation. Fields are accessed through `PodU64::get()` which reads little-endian bytes:
```rust
impl ZeroCopyDeref for EscrowAccount {
    type Target = EscrowAccountZc;
    const DATA_OFFSET: usize = 1; // discriminator length
}
```

**Re-initialization protection** — `init()` checks the discriminator region is all-zero before writing. All-zero discriminators are banned at compile time, so uninitialized data can never match a valid account:
```rust
pub fn init(self, account: &mut Initialize<Self>, payer: &AccountView, rent: Option<&Rent>)
    -> Result<(), ProgramError>
{
    let existing = unsafe { view.borrow_unchecked() };
    if existing.len() >= 1 {
        if unsafe { *existing.get_unchecked(0) } != 0 {
            return Err(QuasarError::AccountAlreadyInitialized.into());
        }
    }
    // ... create_account CPI, write discriminator + data
}
```

## Compute Units

Both programs implement the same escrow logic and run against the same test harness:

| Instruction | Quasar | Pinocchio (hand-written) | Delta |
|-------------|--------|--------------------------|-------|
| Make        | 9,409  | 9,853                    | -444   |
| Take        | 17,800 | 17,862                   | -62    |
| Refund      | 11,945 | 12,033                   | -88    |

The codegen advantages come from decisions that are tedious to make by hand: byte-level discriminator checks instead of slice comparisons, eliding borrow tracking when the access pattern is statically known, and folding account header arithmetic at compile time.

## Pod Types

Alignment-1 integer wrappers for zero-copy struct fields:

```rust
PodU16, PodU32, PodU64, PodU128
PodI16, PodI32, PodI64, PodI128
PodBool
```

Each is `#[repr(transparent)]` over `[u8; N]`. Arithmetic operators use wrapping semantics in release builds for CU efficiency. Use `checked_add`, `checked_sub`, `checked_mul`, `checked_div` when overflow matters.

## Building

```bash
# Build SBF binary
cargo build-sbf --manifest-path examples/escrow/Cargo.toml

# Run tests (prints CU consumption)
cargo test -p quasar-escrow -- --nocapture

# Check workspace
cargo check --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Generate IDL
cargo run -p quasar-idl

# Run Miri UB tests (requires nightly)
rustup +nightly component add miri
MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-symbolic-alignment-check" \
  cargo +nightly miri test -p quasar-core --test miri
```

The `examples/escrow/` directory contains the full reference implementation used for CU benchmarking. `examples/pinocchio-escrow/` contains the hand-written Pinocchio equivalent for comparison.

## Safety

Quasar uses `unsafe` for zero-copy access, raw CPI syscalls, and pointer casts. Every `unsafe` block has a `// SAFETY:` comment explaining the invariant.

### Miri Validation

Every unsafe code path is tested under [Miri](https://github.com/rust-lang/miri) with Tree Borrows and symbolic alignment checking:

```bash
MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-symbolic-alignment-check" \
  cargo +nightly miri test -p quasar-core --test miri
```

The test suite covers 42 patterns including `& → &mut` casts, `copy_nonoverlapping` flag extraction, `MaybeUninit` array initialization, event memcpy, CPI data construction, and remaining accounts pointer arithmetic. All pass clean under Tree Borrows.

### Safety Model

- **Alignment** — ZC companion structs enforce `assert!(align_of::<T>() == 1)` at compile time. Wider-type access (Rent sysvar, instruction data reads) is technically misaligned in the Rust abstract machine but handled natively by the SBF VM — this is the standard approach across all Solana frameworks.
- **Bounds** — account data length is validated once during `AccountCheck::check`. Field access via `Deref` relies on that upstream check — no redundant bounds checking per access.
- **Initialization** — `init()` verifies the discriminator region is all-zero before writing. All-zero discriminators are banned at compile time, so uninitialized data never passes validation.
- **Interior mutability** — `from_account_view_mut` casts `&AccountView` to `&mut Self` (`#[repr(transparent)]`). Mutations go through `AccountView`'s raw pointers to SVM memory — same pattern as Pinocchio.

### Design Choices

- **Explicit discriminators** — discriminators are developer-specified integers, not sha256 hashes. You can read the discriminator from the source code. All-zero discriminators are rejected at compile time.
- **Zero heap allocation** — the `no_alloc!()` macro installs a global allocator that panics on any heap allocation. The entire dispatch → parse → CPI path is provably zero-allocation.
- **Component crate dependencies** — Quasar depends on decomposed `solana-*` component crates (e.g. `solana-address`, `solana-account-view`) instead of the monolithic `solana-program`. This reduces compile times and dependency surface.

## License

MIT
