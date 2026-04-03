# Cross-Instruction Optimization: CPI Wireframe Reuse

## Problem

Under fat LTO (`lto = "fat"`, `codegen-units = 1`), LLVM collapses every Quasar program into a single monolithic `entrypoint` function. Each instruction path gets its own inlined copy of **CPI account construction** — building `CpiAccount` structs from `AccountView` for derive-generated CPIs (`init`, `close`, `sweep`). Each site reconstructs the same 56-byte struct from RuntimeAccount fields.

Measured impact (escrow, 3 instruction paths):
- 42 CpiAccount flag extractions (21 full CpiAccount constructions)
- ~23.6% of `.text` is structurally duplicated CPI code
- ~34.5% of vault `.text` is cold error-path code (zero happy-path CU impact but bloats .so)

These duplications exist because the `#[derive(Accounts)]` macro processes each instruction context independently. It has no visibility into what other instruction paths need.

## Approach

Pre-allocate CPI call structures at a fixed stack offset, pre-fill compile-time constants, and populate variable fields during the parse loop using the `raw` RuntimeAccount pointer directly. This eliminates redundant `CpiAccount` reconstruction across derive-generated CPI sites.

The optimization is invisible to users — the `#[derive(Accounts)]` API stays identical.

## Design

### 1. CPI Wireframe Reuse

#### Current flow

`#[account(init)]` generates different CPI sequences depending on the account type:

1. **Plain `Account<T>`**: `init_account` (CreateAccount or Transfer+Assign+resize) → write discriminator (no second CPI)
2. **Token account**: `init_account` → `initialize_account3` (token program CPI)
3. **Mint account**: `init_account` → `initialize_mint2` (token program CPI)
4. **ATA**: single CPI to ATA program (6 accounts, completely different shape — no `init_account`)

All non-ATA paths share the same `init_account` call, which in the common case (fresh account, lamports == 0) executes a `CreateAccount` system CPI:

```rust
// Common path of init_account — generated per init site:
// 1. Build instruction data (52 bytes) — write disc, lamports, space, owner
// 2. Build InstructionAccount[2] — write address pointers + flags
// 3. Build CpiAccount[2] via cpi_account_from_view — 7 fields × 2 accounts
// 4. Call sol_invoke_signed_c
```

For escrow `Make` (which has `init` on escrow + `init_if_needed` on maker_ta_b + vault_ta_a), this construction is repeated for each init site. Across instruction paths, the same CPI shape (CreateAccount with 2 accounts, 52 bytes data) is duplicated.

#### CpiAccount reconstruction cost

`cpi_account_from_view` currently:
1. Reads header u32 from RuntimeAccount (1 load)
2. Shifts right 8 to extract flags (1 instruction)
3. Writes 7 fields into CpiAccount struct (7 stores)
4. Total: ~10-12 sBPF instructions per CpiAccount

But the flags were already validated during `parse_accounts` (the header was checked against `NODUP_MUT_SIGNER` etc.). Re-extracting them is redundant work. And every other CpiAccount field is either a constant offset from the RuntimeAccount's `raw` pointer or a compile-time constant.

#### Wireframe concept

A "wireframe" is a pre-allocated `CpiCall` struct at a fixed offset on the entrypoint's stack frame. Compile-time constants are written once before the dispatch match. Variable fields are populated during the parse loop (which already has the `raw` pointer to each RuntimeAccount).

```
Stack layout:
┌──────────────────────────────────┐  ← frame pointer (r10)
│  AccountView[N] buf              │  ← parse output (existing)
├──────────────────────────────────┤
│  CpiCall<2,52> __wf_create       │  ← wireframe at KNOWN OFFSET
├──────────────────────────────────┤
│  ... rest of stack ...           │
└──────────────────────────────────┘
```

Because the wireframe is at a fixed stack offset, both the parse loop and the instruction body reference it as `[r10 - KNOWN_OFFSET]` — no register needed to hold its address.

#### CpiCall memory layout

The wireframe is manipulated via raw pointer writes, so exact byte offsets matter. `CpiCall<ACCTS, DATA>` is `#[repr(Rust)]` (field order determined by compiler), so the wireframe does NOT use `CpiCall` directly. Instead, it uses a `#[repr(C)]` layout with predictable offsets:

```
Wireframe<ACCTS, DATA> (#[repr(C)]):
  offset 0:                         program_id: *const Address     (8 bytes)
  offset 8:                         accounts:   [InstructionAccount; ACCTS]
                                                 (ACCTS × 16 bytes)
  offset 8 + ACCTS×16:              cpi_accounts: [CpiAccount; ACCTS]
                                                   (ACCTS × 56 bytes)
  offset 8 + ACCTS×16 + ACCTS×56:   data: [u8; DATA]
```

For `create_account` (ACCTS=2, DATA=52):
- `program_id`:     offset 0     (8 bytes)
- `accounts[0]`:    offset 8     (16 bytes — InstructionAccount is address ptr + flags + padding)
- `accounts[1]`:    offset 24    (16 bytes)
- `cpi_accounts[0]`: offset 40   (56 bytes)
- `cpi_accounts[1]`: offset 96   (56 bytes)
- `data[0..52]`:    offset 152   (52 bytes)
- **Total: 204 bytes**

The code generator computes these offsets as `const` expressions from `size_of::<InstructionAccount>()` and `size_of::<CpiAccount>()`, with compile-time assertions that the layout matches expectations.

#### What's pre-filled vs. per-use

For a `create_account` wireframe (`CpiCall<2, 52>`):

**Pre-filled once (before dispatch):**

| Component | Field | Value | Cost |
|-----------|-------|-------|------|
| `data[0..4]` | discriminator | `0u32` | 1 store |
| `accounts[0]` | is_writable, is_signer | true, true | 1 store |
| `accounts[1]` | is_writable, is_signer | true, true | 1 store |
| `cpi_accounts[0]` | rent_epoch | 0 | 1 store |
| `cpi_accounts[0]` | is_signer, is_writable, executable | true, true, false | 1 store |
| `cpi_accounts[1]` | rent_epoch | 0 | 1 store |
| `cpi_accounts[1]` | is_signer, is_writable, executable | true, true, false | 1 store |
| `program_id` | pointer | `&SYSTEM_PROGRAM_ID` | 1 store |

Total: ~8 stores, executed once. If the instruction path doesn't use init, these are wasted (~8 CU) — acceptable.

**Per-use (populated during parse or at CPI call time):**

Given `rawN` (the pointer to the Nth RuntimeAccount, computed by the parse loop):

| Component | Field | Computation | Cost |
|-----------|-------|-------------|------|
| `cpi_accounts[i].address` | pointer | `rawN + 8` | 1 add + 1 store |
| `cpi_accounts[i].lamports` | pointer | `rawN + 72` | 1 add + 1 store |
| `cpi_accounts[i].data_len` | value | `*(rawN + 80)` | 1 load + 1 store |
| `cpi_accounts[i].data` | pointer | `rawN + 88` | 1 add + 1 store |
| `cpi_accounts[i].owner` | pointer | `rawN + 40` | 1 add + 1 store |
| `accounts[i].address` | pointer | `rawN + 8` (same as above) | 1 store |

Total per CpiAccount: 4 adds + 1 load + 6 stores = **11 sBPF instructions**.

Compare to current `cpi_account_from_view`: 1 load + 1 shift + 7 stores + pointer computations = ~12 instructions. Per-account savings are small, but the real win is:

1. **No redundant flag extraction** — flags are compile-time known from the parse validation.
2. **Shared accounts across CPI sites** — if the payer is the same for multiple init calls within one instruction, its CpiAccount is built once.
3. **Instruction data partial reuse** — discriminator (and often owner) pre-filled, only lamports + space patched per use.
4. **Binary deduplication** — the wireframe manipulation code is more compact than full CpiCall construction per site.

#### Per-use instruction data patching

For `create_account` (52 bytes):
```
[0..4]   discriminator = 0  → pre-filled
[4..12]  lamports            → per-use (8 bytes)
[12..20] space               → per-use (8 bytes)
[20..52] owner               → pre-fill if constant across all init paths (e.g., always crate::ID)
```

If owner is the same across all init sites (common — it's usually the program's own ID), only 16 bytes are written per use instead of 52.

#### Wireframe lifecycle

1. **Allocation**: the `#[program]` macro emits a `MaybeUninit<CpiCall<2, 52>>` at the top of the entrypoint function.
2. **Pre-fill**: constant fields are written before the dispatch match.
3. **Population**: during the parse loop, when the macro encounters an account that will be used in a derive-generated CPI, it writes that account's CpiAccount fields into the wireframe using `raw + offset`.
4. **Patching**: at each CPI call site, only the varying fields (different account's CpiAccount, instruction data bytes) are updated.
5. **Invocation**: the wireframe's `invoke_inner` is called, passing it directly to `sol_invoke_signed_c`.
6. **Reuse**: for subsequent CPI calls of the same shape, the wireframe is patched (not rebuilt) and invoked again.

#### Parse loop integration

The parse loop (`parse_accounts`) is generated by `#[derive(Accounts)]` and currently takes `(input: *mut u8, buf: &mut MaybeUninit<[AccountView; COUNT]>)`. The wireframe lives on the `#[program]`-generated stack. These need to connect.

**Approach**: `parse_accounts` gains an optional wireframe pointer parameter. The `#[derive(Accounts)]` macro emits a second method:

```rust
// Existing (unchanged):
pub unsafe fn parse_accounts(
    mut input: *mut u8,
    buf: &mut MaybeUninit<[AccountView; COUNT]>,
) -> Result<*mut u8, ProgramError> { ... }

// New (generated alongside):
pub unsafe fn parse_accounts_with_wireframe(
    mut input: *mut u8,
    buf: &mut MaybeUninit<[AccountView; COUNT]>,
    wf: *mut u8,  // pointer to wireframe storage, or null
) -> Result<*mut u8, ProgramError> {
    // Same parse loop, but when encountering accounts used in init CPIs,
    // also writes CpiAccount fields into the wireframe at known offsets:
    //   wf.byte_add(PAYER_CPI_OFFSET).write(raw + 8)   // address
    //   wf.byte_add(PAYER_CPI_OFFSET+8).write(raw + 72) // lamports
    //   ...etc
    // The offsets are compile-time known from the CpiCall struct layout.
}
```

The `#[program]` macro calls `parse_accounts_with_wireframe` instead of `parse_accounts`, passing the wireframe's stack address. When no wireframe is needed (single-instruction programs, no init), it calls the original `parse_accounts`.

The CPI metadata bridge (`__<type>_cpi_meta!`) tells the derive macro which accounts map to which wireframe slots, so it can generate the correct offset writes.

#### Wireframe invocation

The wireframe bypasses `CpiCall::invoke_inner(&self)` (which requires a fully-initialized `CpiCall`) and instead calls `invoke_raw` directly on the wireframe's constituent arrays. This avoids the "all fields initialized" invariant:

```rust
// Generated at init CPI site:
// At this point, all fields that matter for THIS invocation are written.
// Pre-filled: program_id, flags, rent_epoch, discriminator
// Populated during parse: payer CpiAccount fields
// Patched just now: new account CpiAccount fields + lamports/space/owner in data
let result = unsafe {
    invoke_raw(
        wf_program_id,                          // pre-filled pointer
        wf_accounts.as_ptr(),                    // InstructionAccount array
        2,                                       // account count
        wf_data.as_ptr(),                        // instruction data
        52,                                      // data length
        wf_cpi_accounts.as_ptr(),                // CpiAccount array
        2,                                       // cpi account count
        &signers,                                // PDA signer seeds
    )
};
result_from_raw(result)?;
```

This is safe because all fields referenced by the syscall are written before the call. The `MaybeUninit` regions not used by this CPI shape are never read.

#### CPI metadata bridge

`#[derive(Accounts)]` emits a `macro_rules!` bridge for CPI metadata, following the existing `macro_rules!` bridge pattern (proven by `derive/src/accounts/client.rs`):

```rust
#[doc(hidden)]
#[macro_export]
macro_rules! __make_cpi_meta {
    // Reports init CPI sites for this struct.
    // Each site is either `init` (unconditional) or `init_if_needed` (conditional).
    // The payer field index identifies which account populates cpi_accounts[0].
    // The target field index identifies which account populates cpi_accounts[1].
    (init_sites) => {
        // field "escrow": payer = field(0), target = field(3),
        //     shape = create_account(2, 52), mode = init
        // field "maker_ta_b": payer = field(0), target = field(4),
        //     shape = create_account(2, 52), mode = init_if_needed
        // field "vault_ta_a": payer = field(0), target = field(5),
        //     shape = create_account(2, 52), mode = init_if_needed
    };
}
```

The `mode` distinguishes unconditional `init` from conditional `init_if_needed`. For `init`, the CPI is always executed — the wireframe population during parse is always useful. For `init_if_needed`, the CPI is guarded by an existence check (`is_system_program(owner)`). The wireframe is populated regardless (the payer's CpiAccount fields are written during parse either way), but the CPI invocation is skipped if the account already exists. The ~11 instructions to populate the target account's CpiAccount fields are the only waste in the skip case.

The `#[program]` macro invokes these across all instruction types to determine:
- Which CPI shapes are needed (only `create_account` in the initial implementation)
- How many wireframes to allocate (one per distinct shape)
- Which account field indices map to which wireframe CpiAccount slots

#### Post-CPI correctness and pointer lifetime

After a CPI, the SVM runtime updates the RuntimeAccount data in the input buffer directly (lamports, data_len, owner may change). The wireframe's CpiAccount fields are pointers into that buffer, so they automatically reflect the updated values if the wireframe is reused for a subsequent CPI. This is the same pointer-stability guarantee that `AccountView` already relies on.

**Pointer lifetime guarantee**: the SVM allocates the input buffer (containing all RuntimeAccount headers + data) once per instruction invocation. This buffer remains valid and at the same address for the entire instruction execution, including across CPI calls. The runtime updates fields in-place but never reallocates or moves the buffer. This means pointers stored in the wireframe during the parse loop remain valid through all subsequent CPI invocations within the same instruction. This is the same invariant that `AccountView` (which stores a `*mut RuntimeAccount` into the same buffer) relies on throughout Quasar.

#### Scope

Wireframes target the `CreateAccount` system CPI — the common path of `init_account` (fresh account, lamports == 0):
- `#[account(init)]` on `Account<T>`, token, or mint types → `CreateAccount(2, 52)`
- `#[account(init_if_needed)]` on the same types → conditional `CreateAccount(2, 52)`

This is the highest-value target because `CreateAccount` has a fixed shape (2 accounts, 52 bytes data) identical across all non-ATA init sites.

The following are **not wireframed**:
- **Pre-funded fallback path** of `init_account` (Transfer + Assign + resize) — different CPI shapes, rare edge case, not worth the complexity
- **Follow-up CPIs** after `CreateAccount` (`initialize_account3`, `initialize_mint2`) — these are token program CPIs with different shapes (3+ accounts, different instruction data). Wireframing them would require per-type wireframe shapes with limited reuse.
- **ATA init** — single CPI to ATA program with 6 accounts, completely different shape, no `CreateAccount`
- **`#[account(close = dest)]`** on non-token accounts → framework close (lamport drain + zero-fill + assign, no CPI)
- **`#[account(close = dest)]`** on token accounts → token program CPI close via `TokenClose` trait methods
- **`#[account(sweep = receiver)]`** → token program `transfer_checked` CPI via `TokenCpi` trait methods
- **User-written CPIs** in instruction handler bodies — the macro has no visibility into handler bodies

#### Savings estimate

For the escrow (3 paths, multiple init_if_needed sites):

| Metric | Before | After | Delta |
|--------|--------|-------|-------|
| CpiAccount constructions | 21 (across all paths) | ~12 (shared accounts built once per path) | **-~9 constructions** |
| Per-construction cost | ~12 instructions | ~11 instructions (no flag re-extraction) | **-1 per** |
| Instruction data writes | 52 bytes per init site | 16 bytes per init site (disc + owner pre-filled) | **-36 bytes per** |
| .so (CPI code) | Full construction per site | Compact patch code per site | **significant** |

### 2. Implementation Scope

#### What changes

| Component | Change |
|-----------|--------|
| `derive/src/accounts/mod.rs` | Emit `__CpiMeta` `macro_rules!` bridge with CPI site metadata |
| `derive/src/accounts/client.rs` | New bridge macro generation for CPI metadata (follows existing pattern) |
| `derive/src/program.rs` | Read CPI metadata across instruction types, generate wireframe allocations and pre-fill code |
| `lang/src/entrypoint.rs` | `dispatch!` macro gains pre-dispatch preamble for wireframe pre-fill |
| `lang/src/cpi/mod.rs` | New `populate_wireframe_cpi_account(raw, wireframe_slot)` helper |
| `derive/src/accounts/fields.rs` | Init codegen writes to wireframe instead of constructing fresh CpiCall |

#### What doesn't change

- User-facing `#[derive(Accounts)]` attribute syntax
- User-facing `#[account(init)]`, `seeds`, `bump` syntax
- `CpiCall` public API
- User-written CPI calls in instruction handler bodies
- Off-chain / client code

### 3. Constraints and Edge Cases

1. **`init_if_needed` conditional execution**: the wireframe is allocated regardless. If the account already exists (init not needed), the wireframe's init CPI fields go unused — zero CU cost beyond the pre-fill (~8 instructions). The conditional check happens before the CPI invocation, not before wireframe population.

2. **Multiple init accounts in one instruction**: e.g., `Make` has 3 init/init_if_needed fields. The same `create_account` wireframe is reused sequentially — populate for account A, invoke, patch for account B, invoke, patch for account C, invoke. No additional stack allocation.

3. **Different CPI shapes**: `create_account` (2 accounts, 52 bytes) and token `transfer` (3 accounts, variable data) are different wireframe shapes. Each gets its own wireframe allocation. For most programs this means 1-2 wireframes (~200-400 bytes of stack).

4. **Stack budget**: a `create_account` wireframe is 204 bytes (see CpiCall memory layout above: 8 + 2×16 + 2×56 + 52). SBF provides 4KB per frame (expandable with dynamic frames). Most programs need at most 1 wireframe (204 bytes), leaving ample room for the AccountView buffer (8 bytes per account) and local variables.

5. **Programs with no derive-generated CPIs**: the wireframe machinery is not emitted. `#[program]` detects zero CPI sites across all instruction types and falls back to the existing `parse_accounts` path.

### 4. Validation Plan

All optimizations must be validated empirically against the current master baseline:

| Test | Method | Pass criteria |
|------|--------|---------------|
| Correctness | Run vault + escrow test suites | All tests pass, identical on-chain behavior |
| CU measurement | Compare deposit/make/take/refund CU | CU <= baseline for all paths |
| .so measurement | Compare vault + escrow .so sizes | .so < baseline |
| Disassembly audit | llvm-objdump on optimized binaries | Verify wireframe pattern visible, no redundant CpiAccount construction |
| Miri | Run under Miri with Tree Borrows | No UB from pointer manipulation |
| Edge cases | Test programs with no-init paths, single-instruction programs | Graceful fallback to current behavior |

### 5. Non-Goals

- Optimizing user-written CPIs in instruction handler bodies
- Changing the `CpiCall` or `CpiAccount` public API
- Inline assembly for CPI construction (LLVM handles the offset arithmetic fine)
- Cross-program wireframe sharing (each program is independent)
- PDA derivation optimization (seeds reference parsed account data, cannot be hoisted pre-dispatch)
- Error path compression via shared trampolines (orthogonal, can be done separately)
