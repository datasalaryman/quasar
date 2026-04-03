# CPI Optimization on sBPF: What We Tried, What We Learned

## The Problem

Quasar programs compile to sBPF (Solana's BPF variant) with `lto = "fat"` and `codegen-units = 1`. Under these settings, LLVM collapses the **entire program into a single monolithic `entrypoint` function**. Every CPI (Cross-Program Invocation) call site gets its own inlined copy of:

- Account conversion (`cpi_account_from_view`): 15-17 sBPF instructions per account
- Instruction building (`CInstruction` struct construction): 15-20 instructions per site
- Syscall invocation (`sol_invoke_signed_c`): ~5 instructions per site

For a CPI-heavy program like the escrow (8 CPI sites, 42 account conversions), this means **23.6% of the `.text` section** is structurally duplicated CPI code. Another **22%** is error handling code.

**Goal**: Find optimizations that reduce binary size (`.so`) AND/OR compute units (CU) below the baseline.

---

## Compilation Environment

- **Target**: `sbf-solana-solana` (sBPF v1)
- **LTO**: `lto = "fat"` — all crates merged into one LLVM module
- **Codegen units**: 1 — single compilation thread, maximum optimization scope
- **Opt level**: `opt-level = 3`
- **Toolchain**: Solana platform-tools v1.51 (rustc 1.84.1-dev, LLVM-based)
- **Linker**: `sbf-lld` (Solana's patched LLD)

**Key constraint**: On sBPF, **CU = number of instructions executed**. There's no instruction cache, no branch predictor, no superscalar execution. Each sBPF instruction costs exactly 1 CU (syscalls have additional fixed costs). This means "faster" code on sBPF is literally "fewer instructions executed."

---

## Benchmark Programs

| Program | CPIs | Accounts per CPI | Tests |
|---------|------|-------------------|-------|
| **Vault** | 1 (system transfer) | 2 | deposit, withdraw |
| **Escrow** | 8 (token transfers + close) | 2-3 | make, take, refund |

Vault tests single-CPI overhead. Escrow tests deduplication benefit across many CPI sites.

---

## Results Table

> **Note on baselines**: Experiments 1-8 were measured against an older master snapshot ("old master" below). The codebase has since changed significantly — vault grew from 7K to 11K .so, escrow from 33K to 40K. Experiments 6 and 10-13 were re-measured against current master (commit `5fda2f5`) to produce accurate comparisons. The old experiment numbers (1-5, 7-8) are preserved for relative comparison between experiments but should NOT be compared to current master.

| Experiment | Vault .so | Escrow .so | Deposit CU | Make CU | Take CU | Refund CU |
|---|---|---|---|---|---|---|
| **Master (current, 5fda2f5)** | **11,256** | **39,840** | **1,588** | **21,270** | **29,567** | **17,222** |
| **Exp 6: remove `#[inline(always)]`** | **11,256** | **38,656** | **1,588** | **21,594** | **29,567** | **17,222** |
| Exp 10: linker ICF | 11,256 | 39,840 | 1,588 | 21,270 | 29,567 | 17,222 |
| Exp 11: codegen restructuring | 11,256 | 39,840 | 1,588 | 21,270 | 29,567 | 17,222 |

**Exp 6 vs current master**: Vault is identical. Escrow .so -1,184B but Make CU **+324 regression**. Take/Refund CU unchanged. **Exp 6 is NOT a clear win on current master.**

Experiments 10, 11 had zero effect (identical to master), confirming those approaches are dead ends regardless of codebase version.

### Old baseline experiments (relative comparisons only)

These were measured against an older master snapshot. The absolute numbers don't match current master, but the relative differences between experiments remain informative.

| Experiment | Vault .so | Escrow .so | Deposit CU | Make CU | Take CU | Refund CU |
|---|---|---|---|---|---|---|
| Old master snapshot | 7,232 | 33,456 | 1,669 | 21,717 | 30,117 | 17,538 |
| Sanity: `#[no_mangle]` | 7,360 | 34,448 | — | — | — | — |
| Exp 1: pre-materialized CpiAccounts | 7,656 | 35,304 | 1,742 | 9,172* | 17,225* | 11,184* |
| Exp 2: `black_box` outline | 7,200 | 34,232 | 1,672 | 21,750 | 30,167 | 17,569 |
| Exp 2b: full invoke outline | 7,088 | 35,592 | 1,601 | 21,607 | 29,603 | 17,239 |
| Exp 3: naked asm invoke trampoline | 6,928 | 34,392 | 1,579 | 21,153 | 29,450 | 17,149 |
| Exp 5: type-erase init_cpi_accounts | 11,464 | 36,616 | 1,613 | 21,800 | 29,767 | 17,326 |
| Exp 6 (old measurement) | 6,888 | 33,216 | 1,577 | 21,180 | 29,436 | 17,135 |
| Exp 7: combined (3+6) | 6,928 | 33,112 | 1,579 | 21,192 | 29,450 | 17,143 |
| Exp 8a: expanded invoke trampoline | 7,048 | 33,432 | 1,629 | 21,558 | 29,845 | 17,365 |
| Exp 8b: per-account asm trampoline | — | — | ~2,400 | ~22,000 | ~30,200 | ~17,900 |
| Exp 8c: combined both trampolines | 7,072 | 28,976 | 1,630 | 21,560 | 29,850 | 17,370 |
| Exp 8d: batch init trampoline | 7,072 | 28,656 | 1,627 | 21,558 | 29,845 | 17,365 |
| Exp 8e: `opt-level="z"` | 12,776 | ~45,000 | — | — | — | — |
| Exp 8e: `opt-level="s"` | ~9,500 | ~38,000 | — | — | — | — |
| Exp 12: mega-trampoline (old) | 6,944 | 32,288 | 1,586 | 21,260 | 29,559 | 17,218 |
| Exp 13: stacked winners (old) | 6,944 | 32,288 | 1,586 | 21,260 | 29,559 | 17,218 |

`*` = not directly comparable (different test paths). `~` = approximate (immediately reverted).

**No experiment beat master on BOTH .so AND CU.**

---

## The Experiments

### Round 1: Finding What Works (Experiments 1-8)

---

### Sanity Check: `#[no_mangle]`

**Idea**: Adding `#[no_mangle]` to CPI functions should create real function symbols that aren't inlined.

**Result**: Binary GREW. Symbols were emitted into `.dynsym` (+992B) but LLVM inlined every call site anyway. The symbols were dead code — present but never called. `.text` was identical to baseline.

**Lesson**: On sBPF with fat LTO, **symbol attributes have zero effect on inlining**. `#[no_mangle]`, `#[inline(never)]`, `#[export_name]` — LLVM ignores them all. It treats the entire program as one compilation unit and inlines everything it wants to.

---

### Experiment 1: Pre-materialized CpiAccount Cache

**Idea**: Build all CpiAccounts once at instruction entry, store them, and pass them to all CPI helpers. Avoid re-reading account headers at each CPI site.

**Result**: Binary GREW +5-6%. Since LTO inlines everything, the "shared" cache initialization code was duplicated at each use site — we added code without removing any. Each CPI helper got a `_cached` twin, doubling the API surface.

Worse: the cache broke `init_if_needed` accounts. The cache was built after `parse_accounts`, but `init_if_needed` creates/resizes accounts during parsing. Cached CpiAccounts pointed at stale data for newly created accounts.

**Lesson**: Pre-computation doesn't help when LTO inlines everything. You're adding code, not sharing it. Also beware of cache invalidation with lazy initialization patterns.

---

### Experiment 2: `core::hint::black_box` Outline

**Idea**: Wrap `cpi_account_from_view` in `black_box()` to hide the function pointer from LLVM, forcing a real call.

**Result**: Didn't work. No function symbol appeared. LLVM saw through `black_box` and inlined normally. Binary grew slightly, CU increased slightly.

**Lesson**: `black_box` is a **volatile read barrier, not an inlining barrier**. It prevents value optimization (constant folding, dead code elimination of the value), not call optimization. Under fat LTO, LLVM resolves function pointers at compile time regardless of `black_box`.

---

### Experiment 2b: `black_box` on Full Invoke Path

**Idea**: Same approach but wrapping the entire `invoke_raw` path and packing arguments into a struct.

**Result**: Still failed to outline (no symbol appeared). BUT CU **decreased** -68 to -514 despite the binary growing. The struct packing accidentally changed LLVM's code layout in a way that improved register allocation.

**Lesson**: Even failed experiments can reveal accidental improvements. Code layout changes (struct packing, argument ordering) can influence LLVM's register allocator. But these effects are fragile and unpredictable — not a reliable optimization strategy.

---

### Experiment 3: `#[naked]` Assembly Trampoline

**Idea**: A `#[naked]` function with `naked_asm!()` creates an opaque function body that LLVM literally cannot inline — the body is raw assembly, not LLVM IR.

**Implementation**: A naked trampoline for `sol_invoke_signed_c` that builds the `CInstruction` struct and calls the syscall. All CPI sites call through this single shared trampoline.

**Result**: **SUCCESS — first real win.** CU decreased on all metrics (-90 to -667 vs master). Vault .so decreased -304B. Only 1 `sol_invoke_signed_c` relocation per binary instead of N.

CU savings came from deduplicating the syscall setup code — instead of N inlined copies of the calling convention setup, there's 1 shared trampoline called N times. The call/exit overhead (2 CU per call) was less than the duplicated setup code it replaced.

**Lesson**: **`#[naked]` with `naked_asm!` is the ONLY way to create a function boundary that survives fat LTO on sBPF.** Nothing else works — not `#[inline(never)]`, not `#[no_mangle]`, not `black_box`. Only raw asm is opaque to LLVM.

---

### Experiment 5: Type-Erased `init_cpi_accounts`

**Idea**: Replace the const-generic `init_cpi_accounts<const N: usize>` with a runtime-length version to prevent monomorphization (each unique N generates its own function copy that LTO then inlines separately).

**Result**: `.text` shrank (monomorphization was real), but `.so` grew significantly. The runtime-length version required dynamic dispatch machinery that inflated `.rodata` and `.rel.dyn` sections.

Vault: .text -120B but .so **+4,232B**. Escrow: .text -1,536B but .so +3,160B.

**Lesson**: Type erasure trades `.text` savings for `.rodata`/`.rel.dyn` bloat. On sBPF, the non-code sections can overwhelm the code savings. Only worthwhile for programs with many different CPI arities (4+ unique N values).

---

### Experiment 6: Remove `#[inline(always)]`

**Idea**: Replace `#[inline(always)]` with `#[inline]` on all CPI path functions, letting LLVM decide.

**Result against old master**: Appeared to improve every metric. Vault .so -344B, escrow .so -240B, CU -92 to -681.

**Result against current master**: Mixed. Vault .so identical (11,256). Escrow .so -1,184B (38,656 vs 39,840). But escrow Make CU **regressed +324** (21,594 vs 21,270). Take/Refund CU unchanged. Deposit CU unchanged.

The effect of removing `#[inline(always)]` depends on the surrounding code. On the old codebase it helped everywhere. On the current codebase, LLVM produces worse code for the Make instruction path when given inlining freedom — it makes a different inlining decision that increases Make's executed instruction count.

**Lesson**: `#[inline(always)]` vs `#[inline]` under fat LTO is **context-dependent, not universally better or worse**. LLVM's inlining heuristics react to the entire module's code layout. Changing the surrounding code (other functions, other instructions) can flip whether forced or relaxed inlining produces better results. This is not a reliable optimization lever.

---

### Experiment 7: Combined Best (Exp 3 + Exp 6)

**Idea**: Combine the naked asm invoke trampoline (forced syscall deduplication) with relaxed inlining (optimal LLVM decisions for everything else).

**Result (old baseline)**: Escrow .so 33,112B (best at the time). CU: +2 (Deposit), +12 (Make), +14 (Take), +8 (Refund) vs old Exp 6 measurements. Not re-measured against current master.

The approaches are orthogonal: Exp 6 controls what LLVM does (don't force inlining), Exp 3 controls what LLVM can't do (force syscall deduplication).

**Lesson**: Orthogonal optimizations can stack. But since Exp 6's effect is context-dependent on current master, this combination's real-world value is uncertain without re-measurement.

---

### Experiment 8: Giga Optimization (Five Sub-Experiments)

Built on Exp 7 (relaxed inlining + invoke trampoline). Tested five approaches to push further.

---

#### Exp 8a: Expanded Invoke Trampoline (InvokeArgs Struct Packing)

**Idea**: Pack all 8 arguments of `invoke_raw` into a struct, pass a pointer to the trampoline, have the trampoline build `CInstruction` from the struct.

**Result**: Binary GREW. The caller still packs 9 fields into `InvokeArgs` — structurally identical work to building `CInstruction` directly. Moving stores between functions with zero net deduplication.

**Lesson**: **Argument packing only helps if the packed struct is simpler than what it replaces.** When the arg struct has the same number of fields as the original, you're moving work, not eliminating it.

---

#### Exp 8b: Per-Account `cpi_account_from_view` Trampoline

**Idea**: A naked asm trampoline that converts one `RuntimeAccount` to one `CpiAccount`. Called per-account.

**Result**: CU **catastrophically regressed** (+800-1,000 for escrow). With 15 calls (5 CPIs × 3 accounts), the per-call overhead dominated. Worse: LLVM's inlined version reuses registers across sequential account conversions and interleaves with subsequent code. The per-call trampoline breaks all cross-account optimization.

**Lesson**: **Per-call trampolines are the worst of both worlds.** The function is too small to amortize call overhead, and too frequently called to ignore the per-call cost. LLVM's inlined version wins because it sees all N accounts as a single optimization scope.

---

#### Exp 8c: Combined Both Trampolines

**Result**: Escrow .so 28,976B. Confirmed the two trampolines (batch init + invoke) are roughly additive for .so savings but both add CU overhead independently.

---

#### Exp 8d: Batch `init_cpi_accounts` Trampoline

**Idea**: A single naked asm function that batch-converts an array of `RuntimeAccount` pointers into `[CpiAccount; N]` using an internal loop. Called once per CPI, not once per account. Amortizes call overhead across all N accounts.

**Implementation**: Takes `(ptrs: *const *const RuntimeAccount, out: *mut CpiAccount, count: u64)`. Loop body computes output offset via bit shifts (`i × 56 = (i << 6) - (i << 3)` to avoid missing sBPF multiply instruction), fills 7 CpiAccount fields per iteration.

**Result**: **Best escrow .so ever — 28,656B** (-4,560B vs Exp 6, -13.7%). CU regressed: +50 (Deposit), +378 (Make), +409 (Take), +230 (Refund) vs Exp 6.

The .so savings are massive because the trampoline body (~35 instructions) replaces 5 inlined copies (~175 instructions total). But CU regresses because:
1. Pointer extraction loop in Rust wrapper: ~6 CU
2. Call/exit: 2 CU
3. Asm loop overhead: ~12 CU per account (offset computation, branch, counter)
4. Lost cross-boundary optimization: ~30 CU — LLVM can no longer interleave account conversion with instruction building

**Lesson**: **The fundamental .so/CU tradeoff on sBPF.** Every function call boundary saves binary size (deduplication) but costs CU (call/exit + lost LLVM cross-boundary optimization). This is inherent to the sBPF execution model and cannot be eliminated.

---

#### Exp 8e: Compiler Flags

**`opt-level = "z"` (optimize for size)**: CATASTROPHIC — vault .so nearly doubled to 12,776B. LLVM's `-Oz` outlines code into separate functions for size, but fat LTO re-inlines them. The combination of "optimize for small functions" (which get re-inlined) and "don't optimize for speed" produces strictly worse code.

**`opt-level = "s"`**: Same mechanism, less severe. Still worse than `opt-level = 3` on all metrics. Vault ~9,500B, escrow ~38,000B.

**`strip = true`**: No effect — `cargo build-sbf` already strips everything.

**`panic = "abort"`**: No effect — sBPF hardcodes panic=abort regardless of profile setting.

**Lesson**: **`opt-level = 3` is the only sensible choice on sBPF.** Size-oriented levels ("z", "s") create function boundaries that fat LTO dissolves, resulting in pessimized inlined code. Other flags are either already applied by the Solana toolchain or have no effect.

---

### Round 2: Searching for a Pareto Win (Experiments 9-13)

All branched from `exp/6-remove-inline` (the best CU baseline).

---

### Experiment 9: Codegen Disassembly Analysis

**Type**: Analysis only, no code changes.

Disassembled the Exp 6 vault and escrow binaries with `llvm-objdump -d`. Mapped every sBPF instruction back to Rust source patterns.

**Key findings**:

**Binary structure**: Both programs are a single monolithic `entrypoint` function. The escrow has 448 bytes of pre-entrypoint helper functions (error-result conversion) — the only code that survived as separate functions.

**CPI account conversion**: 42 inlined instances in escrow, each 15-17 instructions (120-136 bytes). Each instance reads a `RuntimeAccount` pointer and writes 7 fields: address ptr (base+8), owner ptr (base+40), lamports ptr (base+72), data_len value (base+80), data ptr (base+88), flags (u32 at base+0 >> 8), rent_epoch (hardcoded 0). **Total: ~5,700 bytes = 19% of .text.**

**No byte-identical sequences**: LLVM varies register assignments and stack offsets at each CPI site based on surrounding context. Even structurally identical operations (two 3-account token transfers) differ in operands. This means **linker-level deduplication (ICF) cannot help**.

**Register pressure is extreme**: 975 total stack operations in escrow. CPI sites show 23-28 `stxdw` spills each. With only 10 registers and 21+ fields to compute (3 accounts × 7 fields), spilling is unavoidable.

**Escrow .text breakdown**:

| Category | Bytes | % of .text |
|----------|------:|----------:|
| CPI account conversion | ~5,700 | 19.0% |
| CPI invocation | ~1,400 | 4.7% |
| **CPI total** | **~7,100** | **23.6%** |
| Account parsing & validation | ~5,200 | 17.3% |
| Error handling & error map | ~6,600 | 22.0% |
| PDA derivation | ~2,400 | 8.0% |
| Data operations | ~3,500 | 11.6% |
| Post-CPI account reloads | ~960 | 3.2% |
| Dispatch + helpers + other | ~4,296 | 14.3% |

**Lesson**: CPI code (23.6%) and error handling (22%) together make up nearly half the binary. But CPI code is on the hot path (affects CU), while error handling is not (only runs on failures, which benchmarks don't exercise). This asymmetry is key: you can't reduce CPI code without the .so/CU tradeoff, but error handling is .so-only overhead.

---

### Experiment 10: Linker ICF (Identical Code Folding)

**Idea**: Pass `--icf=all` to `sbf-lld`. ICF merges identical function bodies post-compilation — zero runtime overhead.

**Result**: **Zero effect.** Byte-identical binaries to Exp 6.

Fat LTO collapses everything into one function. With one function in the binary, there are no pairs of functions for ICF to compare and merge. Both `--icf=all` and `--icf=safe` produced identical output.

**Additional finding**: `[target.sbf-solana-solana]` sections in `.cargo/config.toml` are **not picked up by `cargo build-sbf`**. Must use `RUSTFLAGS` environment variable instead.

**Lesson**: **Linker ICF is fundamentally incompatible with fat LTO.** ICF operates at function granularity, but fat LTO eliminates function boundaries. Any deduplication must happen before or during LTO, not after.

---

### Experiment 11: Codegen-Guided Restructuring

**Idea**: Use Exp 9's disassembly insights to restructure the Rust source for `cpi_account_from_view`. Three variants:

1. **Base pointer arithmetic**: Compute base pointer once, use fixed offsets
2. **`ptr::write` construction**: Write fields via `ptr::write` to `MaybeUninit` instead of struct literal + transmute
3. **Field write ordering**: Write fields in ascending memory offset order

**Result**: **All three produced byte-identical binaries.** Zero change to `.text`, `.rodata`, or `.rel.dyn`.

**Lesson**: **LLVM normalizes all source-level patterns under fat LTO.** The optimizer sees through Rust's pointer abstractions, struct construction methods, and field ordering — at LLVM IR level, they all become the same sequence of GEPs (GetElementPtr) and stores. The only ways to change codegen are: (a) change the algorithm (different computation), or (b) use `#[naked]` asm (bypass LLVM entirely).

---

### Experiment 12: Mega-Trampoline

**Idea**: Combine the entire 3-account CPI into a single `#[naked]` asm function: account conversion + CInstruction build + `sol_invoke_signed_c` call. One function call per CPI instead of separate init + invoke steps.

**Implementation**: `__quasar_mega_cpi` takes a pointer to `MegaCpiArgs` (3 account pointers + program_id + instruction accounts + data + signers). Uses 208 bytes of stack: 168B for `CpiAccount[3]` (3 × 56B) + 40B for `CInstruction`. Unrolled account conversion (no loop — hardcoded for 3 accounts). Only activated for `ACCTS == 3`; 2-account CPIs fall through to standard path.

**Result**: Escrow .so **32,288B** (-928B vs Exp 6). Vault .so 6,944 (+56B due to `views` field). CU: +9 (Deposit), +80 (Make), +123 (Take), +83 (Refund) vs Exp 6.

The mega-trampoline eliminates intermediate returns between account conversion and syscall. The standard path must maintain the CpiAccount array in the caller's stack frame across the init→invoke boundary; the mega-trampoline builds everything in its own stack frame.

**Lesson**: Multi-step asm trampolines beat separate trampolines because they eliminate the caller's obligation to marshal intermediate data. But they still can't beat LLVM's fully-inlined version for CU — the generic asm must handle all callers identically, while LLVM's inlined version uses caller-specific constants.

---

### Experiment 13: Stacked Winners

**Idea**: Combine Exp 12's mega-trampoline with Exp 3/7's invoke trampoline (for non-mega paths).

**Result**: Net negative. Escrow .so improved only 8B over Exp 12 alone. Vault .so grew +40B. CU slightly worse.

The mega-trampoline already handles all 3-account CPIs (the high-frequency path). The remaining 2-account paths have a single call site — there's nothing to deduplicate with a trampoline.

**Lesson**: **Trampolines only help with 2+ call sites.** A trampoline wrapping a single call site is pure overhead — it adds call/exit instructions without deduplicating anything.

---

## What We Learned

### The Central Discovery: The .so/CU Tradeoff Is Fundamental

On sBPF with fat LTO, **you cannot reduce .so without increasing CU** (and vice versa). This isn't a tooling limitation — it's a consequence of the execution model:

- **CU = instructions executed.** No caches, no branch prediction, no superscalar. Each instruction costs exactly 1 CU.
- **Fat LTO inlines everything.** LLVM sees the entire program and produces globally optimal inline code.
- **Function boundaries cost CU.** The `call` instruction (1 CU) + `exit` instruction (1 CU) + register save/restore + loss of cross-boundary optimization.
- **Function boundaries save .so.** Shared trampoline body instead of N inlined copies.

These two forces directly oppose each other. Every experiment that reduced .so increased CU. No experiment beat current master on both metrics simultaneously.

### The 15 Specific Lessons

1. **`#[inline(always)]` vs `#[inline]` is context-dependent under fat LTO.** On one codebase version, removing `#[inline(always)]` improved everything. On the current version, it regresses Make CU by +324. LLVM's inlining decisions depend on the entire module — surrounding code changes can flip the outcome. Neither annotation is universally better.

2. **`#[naked]` with `naked_asm!` is the only real function boundary on sBPF.** Everything else (`#[inline(never)]`, `#[no_mangle]`, `black_box`, function pointer indirection) is ignored by LLVM under fat LTO.

3. **The .so/CU tradeoff is inherent and cannot be eliminated.** It can only be navigated along the Pareto frontier. The optimal point depends on the program's CPI density.

4. **Batch trampolines >> per-call trampolines.** Processing N items in one call (1× overhead) beats calling a function N times (N× overhead). Batch init trampoline: -4.5KB escrow .so. Per-account trampoline: +800 CU regression.

5. **LLVM's inlined code beats hand-written asm for CU.** LLVM knows exact stack offsets, reuses registers across operations, and interleaves code. Generic asm must handle all callers identically.

6. **Argument packing doesn't help when arg count matches the replaced struct.** Moving `CInstruction` build into a trampoline requires `InvokeArgs` with the same cardinality — net zero deduplication.

7. **`core::hint::black_box` does not prevent inlining.** It prevents value optimization, not call optimization.

8. **Pre-computation doesn't help when LTO inlines everything.** Caching intermediate results just adds code — LTO duplicates the cache access at each site.

9. **Type erasure trades `.text` for `.rodata`/`.rel.dyn`.** Eliminating monomorphization saves code but inflates data sections. Only worthwhile for high-arity programs.

10. **`opt-level = "z"` and `"s"` are catastrophically bad on sBPF.** They optimize for function boundaries that fat LTO dissolves, producing worse inlined code. `opt-level = 3` is the only sensible choice.

11. **CU and binary size are correlated but not identical.** Some experiments (2b) accidentally decreased CU while increasing binary size through code layout changes.

12. **LLVM normalizes all source-level patterns under fat LTO.** Pointer arithmetic style, struct construction method, field ordering — all produce identical codegen. The only way to change codegen is to change the algorithm or use `#[naked]` asm.

13. **Linker ICF is useless under fat LTO.** One function means nothing to fold.

14. **Mega-trampolines (multi-step asm) beat separate trampolines.** Combining account init + invoke eliminates intermediate data marshaling between caller and callee.

15. **Trampolines only help with 2+ call sites.** A trampoline for a single call site is pure overhead.

---

## The Pareto Frontier

Against current master (escrow .so / Refund CU shown):

```
  Master is the CU baseline. No experiment improved CU.
  Moving right trades CU for .so savings.

  Master ──── Exp 6 ──── Exp 12 ──── Exp 8c ──── Exp 8d
  .so: 39,840   38,656     32,288†     28,976†     28,656†
  CU:  17,222   17,222     17,218†     17,370†     17,365†
  Δ.so:   0     -1,184      —           —           —
  ΔCU:    0        0        —           —           —
```

`†` = measured against old master snapshot, not re-verified against current master. Relative differences between these experiments are valid but absolute numbers may shift.

Exp 6 saves 1,184B on escrow .so with no Refund CU change, but **regresses Make CU by +324**. No experiment improves all metrics simultaneously.

---

## Recommendations

**No universal wins exist.** Current master already represents a local optimum. The experiments confirmed that every approach either has zero effect or trades .so for CU.

**If escrow .so reduction is worth a Make CU regression**:
- Removing `#[inline(always)]` (Exp 6) saves 1,184B escrow .so but costs +324 Make CU
- Needs to be re-evaluated whenever surrounding code changes — the effect is not stable

**If larger .so reduction is worth broader CU regression** (needs re-measurement on current master):
- Mega-trampoline (Exp 12) — largest .so savings with smallest CU cost in old measurements
- Batch init trampoline (Exp 8d) — maximum .so savings but significant CU cost

**Dead ends (do not revisit)**:
- Pre-materialized CpiAccount cache (Exp 1) — breaks `init_if_needed`, grows binary
- `black_box` outlining (Exp 2/2b) — doesn't work on sBPF
- Per-account asm trampoline (Exp 8b) — worst of both worlds
- Expanded invoke trampoline (Exp 8a) — same-cardinality packing, no benefit
- `opt-level = "z"` or `"s"` (Exp 8e) — catastrophically bad on sBPF
- Linker ICF (Exp 10) — zero effect under fat LTO
- Source-level codegen restructuring (Exp 11) — LLVM normalizes everything

---

## Branches

| Branch | Description | Status |
|--------|-------------|--------|
| `exp/1-prematerialized` | Pre-materialized CpiAccount cache | Failed |
| `exp/2-blackbox` | black_box outline attempt | Failed |
| `exp/2b-full-invoke` | black_box on full invoke path | Failed (accidental CU win) |
| `exp/3-naked-asm` | Naked asm invoke trampoline | Success (first real boundary) |
| `exp/5-type-erase` | Type-erased init_cpi_accounts | .text win, .so loss |
| `exp/6-remove-inline` | Remove #[inline(always)] | Mixed (escrow .so down, Make CU up) |
| `exp/7-combined-best` | Exp 3 + Exp 6 | Good balance |
| `exp/8-giga` | Five sub-experiments | Exp 8d = best .so |
| `exp/10-linker-icf` | Linker ICF attempt | Zero effect |
| `exp/12-mega-trampoline` | Full CPI in one asm function | Best balance |
| `exp/13-stacked-winners` | Combination attempts | No improvement over Exp 12 |

---

## Deep Dive: The ASM Trampoline Approach

The naked asm trampoline was the most technically interesting direction we explored. It's the only technique that actually creates a function boundary LLVM can't dissolve. This section explains exactly how it works at the instruction level, why it fails to win on CU, and what would need to change for it to succeed.

### Why Trampolines Are Needed

Under fat LTO, LLVM merges all Rust code into a single LLVM IR module and runs whole-program optimization. Every function marked `#[inline]`, `#[inline(always)]`, or even `#[inline(never)]` gets absorbed into the monolithic `entrypoint`. The sBPF backend then emits one giant function.

This is great for CU — LLVM can see the entire program and make globally optimal register allocation, instruction scheduling, and constant propagation decisions. But it means every CPI site gets its own copy of the account conversion + instruction build + syscall code. For a program with 8 CPI sites, that's 8 copies.

The only construct LLVM cannot inline is a `#[naked]` function with `naked_asm!()`. The body is raw target assembly, not LLVM IR — there's nothing to "inline" because there's no IR representation to splice into the caller. LLVM must emit a `call` instruction to reach it.

### The sBPF Calling Convention

sBPF has 11 registers:
- **r0**: return value
- **r1-r5**: function arguments (caller-saved)
- **r6-r9**: callee-saved
- **r10**: frame pointer (read-only)

A function call works like this:
```
caller:
    mov64 r1, <arg1>      ; set up arguments
    mov64 r2, <arg2>
    call <target>          ; pushes return address, jumps to target
    ; r0 now contains return value
    ; r1-r5 are clobbered
    ; r6-r9 are preserved

target:
    ; r1-r5 contain arguments
    ; can use r6-r9 but must save/restore them
    ; r10 points to callee's stack frame
    mov64 r0, <result>
    exit                   ; pops return address, jumps back
```

**Cost of a function call**: The `call` instruction itself is 1 CU. The `exit` is 1 CU. Total minimum overhead: **2 CU per call**. But the real cost is higher because:
1. The caller must place all data the trampoline needs into either r1-r5 (5 registers max) or a stack struct (extra stores)
2. The trampoline must load everything from those arguments (extra loads)
3. LLVM can't optimize across the boundary — it can't reuse a register that was computed before the call and needed after it, because r1-r5 are clobbered
4. The trampoline handles all callers generically — it can't exploit caller-specific constants or stack layout

### The Three Trampoline Variants

We tried three increasingly ambitious trampolines, each moving more work behind the `call` boundary:

#### Variant 1: Invoke Trampoline (Exp 3)

The simplest — only wraps the `sol_invoke_signed_c` syscall:

```asm
__quasar_cpi_invoke:
    call sol_invoke_signed_c    ; forward r1-r5 directly to syscall
    exit                        ; return syscall result in r0
```

**2 instructions. 16 bytes.**

The caller still builds `CInstruction` on its stack and converts all accounts inline. The trampoline just deduplicates the syscall call site. Since `sol_invoke_signed_c` takes exactly 5 arguments (matching sBPF's 5 arg registers), the trampoline is a perfect pass-through — zero argument marshaling overhead.

**What it deduplicates**: Each CPI site had its own `call sol_invoke_signed_c` + the 5-register setup for it. The trampoline replaces N copies with 1 body + N `call __quasar_cpi_invoke` sites. Net savings: (N-1) × ~10 instructions of syscall setup. For escrow (N=8): ~70 instructions = ~560 bytes.

**Why CU improved (old baseline)**: The 2 CU call/exit overhead was less than the duplicated syscall setup LLVM emitted at each site. But this was on the old codebase — on current master, the effect may differ.

#### Variant 2: Batch Init Trampoline (Exp 8d)

Moves account conversion behind the call boundary. A single function converts an array of N `RuntimeAccount` pointers into N `CpiAccount` structs:

```asm
__quasar_init_cpi_accounts:
    ; r1 = *const [*const RuntimeAccount; N]
    ; r2 = *mut [CpiAccount; N]
    ; r3 = N (count)
    mov64 r4, 0                 ; loop index

loop:
    jge r4, r3, done            ; if index >= count, exit

    ; Load RuntimeAccount pointer: ptrs[i]
    mov64 r0, r4
    lsh64 r0, 3                 ; r0 = i * 8 (pointer size)
    add64 r0, r1
    ldxdw r5, [r0 + 0]         ; r5 = ptrs[i]

    ; Compute output offset: i * 56 = (i << 6) - (i << 3)
    mov64 r0, r4
    lsh64 r0, 6                 ; r0 = i * 64
    mov64 r6, r4
    lsh64 r6, 3                 ; r6 = i * 8
    sub64 r0, r6                ; r0 = i * 56
    add64 r0, r2                ; r0 = &out[i]

    ; Fill 7 CpiAccount fields from RuntimeAccount:
    mov64 r6, r5
    add64 r6, 8                 ; &RuntimeAccount.address
    stxdw [r0 + 0], r6         ; CpiAccount.key = &address

    mov64 r6, r5
    add64 r6, 72               ; &RuntimeAccount.lamports
    stxdw [r0 + 8], r6         ; CpiAccount.lamports = &lamports

    ldxdw r6, [r5 + 80]        ; RuntimeAccount.data_len (value)
    stxdw [r0 + 16], r6        ; CpiAccount.data_len = data_len

    mov64 r6, r5
    add64 r6, 88               ; &RuntimeAccount.data[0]
    stxdw [r0 + 24], r6        ; CpiAccount.data = &data

    mov64 r6, r5
    add64 r6, 40               ; &RuntimeAccount.owner
    stxdw [r0 + 32], r6        ; CpiAccount.owner = &owner

    mov64 r6, 0
    stxdw [r0 + 40], r6        ; CpiAccount.rent_epoch = 0

    ldxw r6, [r5 + 0]          ; read 4-byte header as u32
    rsh64 r6, 8                ; drop borrow_state byte, keep flags
    stxdw [r0 + 48], r6        ; CpiAccount.flags = flags

    add64 r4, 1                ; i++
    ja loop

done:
    exit
```

**~35 instructions per iteration + 6 instructions loop overhead.**

The caller extracts raw `RuntimeAccount` pointers from `AccountView` references into a temporary array, then calls the trampoline once. The trampoline processes all N accounts in a loop.

**What it deduplicates**: The per-account conversion code (15-17 instructions × N accounts) is replaced by a single shared function body. For escrow with 42 account conversions across 8 CPI sites: replaces ~714 inlined instructions with ~35 (body) + 8 (calls) = ~43 instructions of unique code. Massive .so savings.

**Why CU regresses**: The trampoline loop has overhead LLVM's inlined version doesn't:

| Cost source | Per-account CU | Notes |
|---|---|---|
| Input pointer load | 4 | `i*8`, add, load from array |
| Output offset computation | 5 | `(i<<6) - (i<<3)`, add to base |
| Loop control | 2 | increment + conditional branch |
| **Loop overhead total** | **11** | Per iteration |
| Account conversion | 17 | Same as LLVM's inline version |
| **Total per account** | **28** | |

LLVM's inlined version does **15-17 instructions per account** because it:
- Knows the array index at compile time (no pointer arithmetic)
- Knows the output offset at compile time (hardcoded stack slot)
- Doesn't need loop control
- Can reuse registers across accounts (e.g., keep the `RuntimeAccount` base pointer in a register across all 3 accounts of a CPI)

So for a 3-account CPI: trampoline = 84 CU, inline = ~48 CU. That's **+36 CU per CPI site**, or +288 CU for 8 escrow CPI sites.

Plus the Rust wrapper adds ~6 CU to extract raw pointers from `AccountView` into the temporary array, and 2 CU for call/exit. Total per-CPI-site overhead: ~44 CU.

#### Variant 3: Mega-Trampoline (Exp 12)

The most ambitious — performs the ENTIRE CPI operation in one asm function: account conversion + CInstruction build + sol_invoke_signed_c call. Hardcoded for exactly 3 accounts (no loop).

The caller packs all inputs into a `MegaCpiArgs` struct (80 bytes):
```
MegaCpiArgs layout:
  offset  0: account_ptrs[0]  (*const RuntimeAccount)
  offset  8: account_ptrs[1]
  offset 16: account_ptrs[2]
  offset 24: program_id       (*const Address)
  offset 32: instruction_accounts  (*const InstructionAccount)
  offset 40: instruction_accounts_len  (u64)
  offset 48: data             (*const u8)
  offset 56: data_len         (u64)
  offset 64: signers          (*const Signer)
  offset 72: signers_len      (u64)
```

The trampoline uses 208 bytes of its own stack:
```
Stack layout:
  r10 - 208 to r10 - 168: CInstruction (40 bytes)
  r10 - 168 to r10 - 112: CpiAccount[0] (56 bytes)
  r10 - 112 to r10 -  56: CpiAccount[1] (56 bytes)
  r10 -  56 to r10 -   0: CpiAccount[2] (56 bytes)
```

The asm is fully unrolled — 3 copies of the 17-instruction account conversion sequence with hardcoded offsets, then 10 instructions for CInstruction build, then 7 for syscall setup + call:

```asm
__quasar_mega_cpi:
    mov64 r7, r1                  ; save args pointer

    ; === Account 0 → stack at r10-168 ===
    ldxdw r5, [r7 + 0]           ; r5 = account_ptrs[0]
    mov64 r6, r5
    add64 r6, 8                  ; &key
    stxdw [r10 - 168], r6
    mov64 r6, r5
    add64 r6, 72                 ; &lamports
    stxdw [r10 - 160], r6
    ldxdw r6, [r5 + 80]         ; data_len value
    stxdw [r10 - 152], r6
    mov64 r6, r5
    add64 r6, 88                 ; &data
    stxdw [r10 - 144], r6
    mov64 r6, r5
    add64 r6, 40                 ; &owner
    stxdw [r10 - 136], r6
    mov64 r6, 0
    stxdw [r10 - 128], r6       ; rent_epoch = 0
    ldxw r6, [r5 + 0]
    rsh64 r6, 8
    stxdw [r10 - 120], r6       ; flags

    ; === Account 1 → stack at r10-112 (same pattern) ===
    ; ... 17 instructions ...

    ; === Account 2 → stack at r10-56 (same pattern) ===
    ; ... 17 instructions ...

    ; === Build CInstruction at r10-208 ===
    ldxdw r6, [r7 + 24]
    stxdw [r10 - 208], r6       ; program_id
    ldxdw r6, [r7 + 32]
    stxdw [r10 - 200], r6       ; accounts ptr
    ldxdw r6, [r7 + 40]
    stxdw [r10 - 192], r6       ; accounts_len
    ldxdw r6, [r7 + 48]
    stxdw [r10 - 184], r6       ; data ptr
    ldxdw r6, [r7 + 56]
    stxdw [r10 - 176], r6       ; data_len

    ; === Call sol_invoke_signed_c ===
    mov64 r1, r10
    add64 r1, -208               ; r1 = &CInstruction
    mov64 r2, r10
    add64 r2, -168               ; r2 = &CpiAccount[0]
    mov64 r3, 3                  ; r3 = 3 accounts
    ldxdw r4, [r7 + 64]         ; r4 = signers ptr
    ldxdw r5, [r7 + 72]         ; r5 = signers_len
    call sol_invoke_signed_c
    exit
```

**~75 instructions total. 600 bytes.** Shared across all 3-account CPI sites.

**What it deduplicates**: Everything — account conversion, CInstruction build, and syscall call. For escrow with six 3-account CPI sites, replaces ~6 × (51 account conversion + 15 invoke + 5 syscall) = ~426 inlined instructions with 75 (body) + 6 (calls) = 81 instructions.

**Why CU still regresses**: The caller must pack the `MegaCpiArgs` struct (10 stores = ~10 CU), plus 2 CU for call/exit, plus the trampoline body executes 75 instructions per call vs LLVM's ~71 inlined. The net overhead per CPI is ~16 CU.

But the deeper issue is what the trampoline CAN'T do:

1. **No register reuse across the boundary.** LLVM's inlined version might compute `self.program_id` once and keep it in a register across multiple field stores. The trampoline starts fresh every call — it loads every field from the args struct individually.

2. **No interleaving with caller code.** LLVM's inlined version can interleave CpiAccount stores with CInstruction stores and syscall setup, using the same registers for multiple purposes. The trampoline does them strictly sequentially.

3. **No constant propagation.** If the caller knows `data_len` is always 9 (a token transfer), LLVM can use `mov64 r6, 9` instead of loading from memory. The trampoline always loads from the args struct.

4. **Double-storing shared values.** The caller stores `program_id`, `data`, `data_len`, etc. into `MegaCpiArgs`, then the trampoline loads them and stores them into `CInstruction`. That's 2 stores + 1 load per field that LLVM's inline version handles with 1 store.

### Why Every Trampoline Variant Loses CU

The CU loss comes from three sources, in order of impact:

**1. Lost LLVM cross-boundary optimization (~60-70% of the regression)**

This is the big one. When everything is inlined, LLVM sees the entire CPI operation as one block: account conversion → CInstruction build → syscall. It can:
- Compute a value once and use it in all three phases (e.g., the program_id pointer)
- Allocate registers globally across all phases
- Schedule instructions to minimize pipeline stalls (even though sBPF has no pipeline, this reduces register spills)
- Dead-code-eliminate stores that are overwritten later

A trampoline fractures this optimization scope. The caller and trampoline each get their own local optimization, but neither can see the other's code. Values that cross the boundary must go through memory (the args struct), adding load/store pairs that LLVM would have kept in registers.

**2. Argument marshaling overhead (~20-25% of the regression)**

The trampoline takes a pointer to a struct. The caller must construct that struct. For the mega-trampoline:
- Caller stores 10 fields into `MegaCpiArgs` on its stack: ~10 instructions
- Trampoline loads those 10 fields: ~10 instructions
- LLVM's inline version doesn't need these 20 instructions — the values are already in registers or at known stack offsets

For the batch init trampoline:
- Caller extracts N `RuntimeAccount` pointers into a temporary array: ~2N instructions
- Trampoline loads them from the array: ~4N instructions (index computation + load)
- LLVM's inline version uses the `AccountView` references directly — 0 extra instructions

**3. Call/exit overhead (~10-15% of the regression)**

2 CU per call. For the invoke trampoline (8 calls in escrow): 16 CU. For the mega-trampoline (6 calls for 3-account CPIs): 12 CU. For the batch init trampoline (8 calls): 16 CU.

This is the smallest contributor but it's an absolute floor — you can never call a function for less than 2 CU on sBPF.

### What Would Need to Change

For asm trampolines to win on BOTH .so and CU, one or more of these would need to be true:

#### 1. Cheaper function calls on sBPF

If `call` + `exit` cost 0 CU (or were somehow free), the overhead floor drops. The Solana runtime could implement this — function calls within the same program could be treated as intra-function jumps with no CU charge. This exists in other VMs (EVM's `JUMP` is cheaper than `CALL`).

**Status**: Would require Solana protocol changes. Not in any known roadmap.

#### 2. Callee-saved register windows

If sBPF had register windows (like SPARC) or a cheaper save/restore mechanism, the cost of crossing a call boundary would drop. Currently, if the caller has values in r1-r5, they must be spilled before the call and reloaded after — that's potentially 10 extra instructions.

**Status**: Would require sBPF ISA changes. SBFv2 doesn't add this.

#### 3. A CPI-specific syscall with a simpler ABI

The current `sol_invoke_signed_c` requires the caller to construct two complex structs (`CInstruction` + `CpiAccount[]`) in a very specific layout. If the runtime instead accepted raw `RuntimeAccount` pointers directly — "invoke this program with these accounts, I'm pointing at the runtime's own memory" — the entire account conversion step (19% of escrow .text, the biggest deduplication target) would become unnecessary.

Something like:
```
sol_invoke_direct(
    program_id: *const Address,
    account_indices: *const u8,     // just indices into the runtime's account table
    account_count: u64,
    data: *const u8,
    data_len: u64,
    signers: *const [*const [u8]],
    signers_len: u64,
) -> u64
```

This would eliminate ~5,700 bytes (19%) of escrow .text AND the corresponding CU cost — no `CpiAccount` construction at all, inline or otherwise.

**Status**: Would require Solana runtime API changes. The current C ABI exists for historical compatibility with C programs.

#### 4. LLVM sBPF-specific function merging pass

An LLVM pass that runs AFTER fat LTO inlining could detect near-identical instruction sequences within the monolithic function and extract them into shared subfunctions. This is what ICF does at the linker level, but it would need to work within a single function — more like a "procedure abstraction" or "code factoring" pass.

The challenge: LLVM's register allocator assigns different registers at each CPI site, so the instruction sequences aren't identical. The pass would need to normalize register assignments, detect structural equivalence, and factor out the common subsequence with a calling convention that remaps registers.

**Status**: Not implemented in LLVM. Academic papers exist on "procedure abstraction" for code compression, but nobody has implemented it for sBPF.

#### 5. A way to make inline code smaller without making it slower

This is the holy grail — reduce the per-site instruction count without adding a function boundary. Possible approaches that haven't been explored:

**a. Pre-computed CpiAccount stored alongside RuntimeAccount**: If the Solana runtime pre-computed the `CpiAccount` struct when setting up the `RuntimeAccount` memory, programs could just read a pointer instead of computing 7 fields per account. This eliminates the conversion entirely.

**b. Compressed CpiAccount**: If `SolAccountInfo` didn't need `rent_epoch` (always 0 since rent exemption became mandatory) and packed the flags into a smaller representation, each account conversion would be fewer instructions. Saving 2-3 instructions × 42 conversions = ~84-126 fewer executed instructions.

**c. Combined account+instruction passing**: Instead of separate `CpiAccount[]` and `InstructionAccount[]` arrays, a single struct containing both sets of data per account would reduce the number of pointer stores per CPI.

**Status**: All require Solana runtime/ABI changes.

### Summary

The asm trampoline approach works exactly as designed — it's the only way to create a real function boundary on sBPF. The technique is sound. But the fundamental economics don't add up: the cost of crossing a call boundary on sBPF (argument marshaling + lost optimization + call/exit overhead) exceeds the CU benefit of code deduplication.

The trampoline is a tool for trading CU for .so. It's useful when binary size is the binding constraint (e.g., hitting the 10KB program account limit, or minimizing deployment cost). But it cannot improve CU because LLVM's whole-program optimization, given full visibility, produces better code than any hand-written generic function can.

The approaches that COULD make trampolines CU-positive all require changes outside the program's control: cheaper function calls in the sBPF runtime, a simpler CPI ABI with fewer fields to marshal, or an LLVM pass that can factor identical code within a single function. None of these are on the immediate horizon.

---

## Conclusion

After 13 experiments across two rounds, we found **no optimization that beats current master on both .so and CU simultaneously**.

Current master (commit `5fda2f5`): vault 11,256B .so / 1,588 CU deposit, escrow 39,840B .so / 21,270 Make CU / 29,567 Take CU / 17,222 Refund CU.

The .so/CU tradeoff is not a bug in our approach or a limitation of the tooling. It is a **fundamental property of sBPF with fat LTO**: the compiler produces globally optimal inline code, and any attempt to deduplicate that code introduces function call overhead that cannot be optimized away.

Even the one change that initially appeared universally beneficial — removing `#[inline(always)]` — turned out to be context-dependent: it regresses Make CU on the current codebase while improving escrow .so. LLVM's inlining decisions under fat LTO depend on the entire module, making the effect of annotation changes unpredictable across code versions.

The CPI code path is at the LLVM optimality frontier for the current architecture. Further improvements would require changes to the sBPF execution model itself (e.g., cheaper function calls, SIMD-style batch operations) or to the Solana runtime's CPI ABI (e.g., fewer fields per `SolAccountInfo`, combined account+instruction passing).
