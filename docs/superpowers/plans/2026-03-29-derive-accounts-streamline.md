# Derive Accounts Architectural Streamline

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the root-cause architectural problems in `derive/src/accounts/` — stringly-typed type classification repeated 25x, scattered optional handling (#101), duplicated init codegen, inconsistent address comparisons, and redundant branching for optional fields — plus runtime fixes in `system.rs` and `account.rs`. Additionally harden security: bounds checks on BUMP_OFFSET reads, dup indices, instruction arg Vec parsing, discriminator writes, dynamic account Vec parsing, PDA seed counts, and `validate_ata` token_program verification for the Interface path.

**Architecture:** The central fix is a `FieldKind` enum that classifies each field's wrapper type ONCE, replacing ~25 independent `extract_generic_inner_type` + string-matching sites with exhaustive `match` statements. Optional account handling is restructured to sentinel-first: determine Some/None at parse time, then apply ALL standard checks for present accounts. The dup-aware path collapses 2-3 flag checks into a single masked u32 comparison. All 32-byte address comparisons use `keys_eq` (word-wise u64 short-circuit) instead of `PartialEq`/`memcmp`. Optional field checks are grouped per-field (single `if let Some` block) instead of scattered across 4 category vectors. Init codegen is extracted with a shared CPI body and context struct, fixing the `needs_rent` false-positive bug. A `debug_checked` codegen helper DRYs the repeated debug/non-debug check pattern. Security hardening adds bounds checks on BUMP_OFFSET reads, dup indices, instruction/dynamic Vec multiplies (`checked_mul`), discriminator writes, PDA seed counts, and `validate_ata` token program verification.

**Tech Stack:** Rust proc-macro (`syn`, `quote`, `proc_macro2`), Solana runtime types (`solana-account-view`)

**Verification:** `cargo check` on the workspace after each task (the derive crate has no unit tests — programs in `tests/programs/` and `examples/` exercise all codegen paths via compilation).

---

## File Structure

```
derive/src/accounts/
├── attrs.rs           (unchanged)
├── client.rs          (unchanged)
├── field_kind.rs      (NEW: FieldKind enum + classify() + precomputed flags)
├── init.rs            (NEW: init codegen with shared CPI body + InitContext)
├── fields.rs          (REWRITE: main loop uses FieldKind, validation/construction via match)
├── mod.rs             (MODIFY: sentinel-first optional in parse_steps, pass program_id)

lang/src/cpi/
├── system.rs          (MODIFY: add init_account to Program<System>, remove dead method)

lang/src/accounts/
├── account.rs         (MODIFY: close() zeros Discriminator::DISCRIMINATOR.len() not hardcoded 8)

lang/src/entrypoint.rs (MODIFY: pass __program_id to parse_accounts)
lang/src/pda.rs        (MODIFY: runtime bounds check on seeds.len())

derive/src/account/
├── dynamic.rs         (MODIFY: checked_mul for Vec byte length in write/parse/offset codegen)

spl/src/
├── validate.rs        (MODIFY: validate_ata verifies token_program is SPL_TOKEN or TOKEN_2022)
```

---

### Task 1: Create `field_kind.rs` — the FieldKind enum and classification

**Files:**
- Create: `derive/src/accounts/field_kind.rs`
- Modify: `derive/src/accounts/mod.rs` (add `mod field_kind;`)

This is the foundation. Every downstream task depends on it.

The enum classifies the wrapper type (Account, InterfaceAccount, Program, Interface, Sysvar, SystemAccount, Signer, UncheckedAccount). The `classify()` function strips references, unwraps `Option<T>`, and matches on the outermost path segment. A companion `FieldFlags` struct precomputes `is_signer`, `is_writable`, `is_executable` from the field kind + attrs — replacing the triple-computation in `determine_nodup_constant`, `compute_header_expected`, and the dup-aware path.

- [ ] **Step 1: Create `field_kind.rs`**

```rust
//! Field type classification for `#[derive(Accounts)]`.
//!
//! Classifies each field's wrapper type ONCE, replacing ~25 independent
//! `extract_generic_inner_type` + string-matching call sites with a single
//! enum that enables exhaustive `match` dispatch.

use {
    crate::helpers::extract_generic_inner_type,
    syn::{Ident, Type},
};

/// The wrapper type of an account field, with inner type where applicable.
///
/// Classified once per field, then used everywhere: validation codegen,
/// field construction, init dispatch, header constants, detected-field
/// scanning, and attribute validation.
pub(super) enum FieldKind<'a> {
    /// `Account<T>` or `&[mut] Account<T>`
    Account { inner_ty: &'a Type },
    /// `InterfaceAccount<T>` or `&[mut] InterfaceAccount<T>`
    InterfaceAccount { inner_ty: &'a Type },
    /// `Program<T>`
    Program { inner_ty: &'a Type },
    /// `Interface<T>`
    Interface { inner_ty: &'a Type },
    /// `Sysvar<T>`
    Sysvar { inner_ty: &'a Type },
    /// `SystemAccount`
    SystemAccount,
    /// `Signer`
    Signer,
    /// Any type not matching above (UncheckedAccount, custom, etc.)
    Other,
}

/// Precomputed header flags for a field. Replaces the triple-computation in
/// `determine_nodup_constant`, `compute_header_expected`, and the dup-aware
/// path in `mod.rs`.
pub(super) struct FieldFlags {
    pub is_signer: bool,
    pub is_writable: bool,
    pub is_executable: bool,
}

/// Strip one layer of `&` / `&mut` from a type.
pub(super) fn strip_ref(ty: &Type) -> &Type {
    match ty {
        Type::Reference(r) => &r.elem,
        other => other,
    }
}

/// Extract the base name (last path segment) of a type.
pub(super) fn type_base_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => type_base_name(&r.elem),
        _ => None,
    }
}

impl<'a> FieldKind<'a> {
    /// Classify a field type. Expects the type AFTER stripping `Option<>` and
    /// references (i.e., pass the "underlying" type).
    pub fn classify(underlying_ty: &'a Type) -> Self {
        // Order matters: check generic wrappers first, then bare types.
        if let Some(inner) = extract_generic_inner_type(underlying_ty, "Account") {
            return FieldKind::Account { inner_ty: inner };
        }
        if let Some(inner) = extract_generic_inner_type(underlying_ty, "InterfaceAccount") {
            return FieldKind::InterfaceAccount { inner_ty: inner };
        }
        if let Some(inner) = extract_generic_inner_type(underlying_ty, "Program") {
            return FieldKind::Program { inner_ty: inner };
        }
        if let Some(inner) = extract_generic_inner_type(underlying_ty, "Interface") {
            return FieldKind::Interface { inner_ty: inner };
        }
        if let Some(inner) = extract_generic_inner_type(underlying_ty, "Sysvar") {
            return FieldKind::Sysvar { inner_ty: inner };
        }
        match type_base_name(underlying_ty).as_deref() {
            Some("SystemAccount") => FieldKind::SystemAccount,
            Some("Signer") => FieldKind::Signer,
            _ => FieldKind::Other,
        }
    }

    pub fn is_executable(&self) -> bool {
        matches!(self, FieldKind::Program { .. } | FieldKind::Interface { .. })
    }

    /// Check if the inner type (for Account/InterfaceAccount) matches any of
    /// the given names.
    pub fn inner_name_matches(&self, names: &[&str]) -> bool {
        let inner = match self {
            FieldKind::Account { inner_ty } | FieldKind::InterfaceAccount { inner_ty } => {
                inner_ty
            }
            _ => return false,
        };
        type_base_name(inner)
            .as_deref()
            .is_some_and(|n| names.contains(&n))
    }

    /// Check if this is a token or mint type (Token, Token2022, Mint, Mint2022).
    pub fn is_token_or_mint(&self) -> bool {
        self.inner_name_matches(&["Token", "Token2022", "Mint", "Mint2022"])
    }

    /// Check if this is a token account (not mint).
    pub fn is_token_account(&self) -> bool {
        self.inner_name_matches(&["Token", "Token2022"])
    }

    /// Check if inner type has a lifetime parameter (dynamic account).
    pub fn is_dynamic(&self) -> bool {
        let inner = match self {
            FieldKind::Account { inner_ty } => inner_ty,
            _ => return false,
        };
        if let Type::Path(tp) = inner {
            if let Some(last) = tp.path.segments.last() {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    return args
                        .args
                        .iter()
                        .any(|arg| matches!(arg, syn::GenericArgument::Lifetime(_)));
                }
            }
        }
        false
    }
}

impl FieldFlags {
    /// Compute header flags from the classified field kind and parsed attrs.
    pub fn compute(
        kind: &FieldKind,
        attrs: &super::attrs::AccountFieldAttrs,
        is_ref_mut: bool,
    ) -> Self {
        let is_signer = matches!(kind, FieldKind::Signer)
            || (attrs.is_init
                && attrs.seeds.is_none()
                && attrs.associated_token_mint.is_none());

        let is_writable = is_ref_mut || attrs.is_mut;

        let is_executable = kind.is_executable();

        FieldFlags {
            is_signer,
            is_writable,
            is_executable,
        }
    }

    /// The NODUP constant name for the no-dup fast path.
    pub fn nodup_constant(&self) -> &'static str {
        if self.is_executable {
            return "NODUP_EXECUTABLE";
        }
        match (self.is_signer, self.is_writable) {
            (true, true) => "NODUP_MUT_SIGNER",
            (true, false) => "NODUP_SIGNER",
            (false, true) => "NODUP_MUT",
            (false, false) => "NODUP",
        }
    }

    /// The expected u32 header value (little-endian: [borrow, signer, writable, exec]).
    pub fn header_constant(&self) -> u32 {
        let mut h: u32 = 0xFF; // byte 0: NOT_BORROWED
        if self.is_signer {
            h |= 0x01 << 8;
        }
        if self.is_writable {
            h |= 0x01 << 16;
        }
        if self.is_executable {
            h |= 0x01 << 24;
        }
        h
    }

}

/// Mask for the dup-aware path: covers all flag bytes (skips borrow_state).
/// Used for single-comparison flag validation in mod.rs Task 3.
pub(super) const FLAG_MASK: u32 = 0xFFFFFF00;

/// DRY codegen helper: emit a check with debug logging on failure.
///
/// In `#[cfg(feature = "debug")]`: logs `msg` with field name, returns Err.
/// In release: just `check_expr?;`
///
/// This replaces the 8-line debug/non-debug pattern repeated ~20 times.
pub(super) fn debug_checked(
    field_name_str: &str,
    check_expr: proc_macro2::TokenStream,
    msg: &str,
) -> proc_macro2::TokenStream {
    quote::quote! {
        #[cfg(feature = "debug")]
        if let Err(e) = #check_expr {
            quasar_lang::prelude::log(&::alloc::format!(#msg, #field_name_str));
            return Err(e);
        }
        #[cfg(not(feature = "debug"))]
        #check_expr?;
    }
}
```

- [ ] **Step 2: Add module declaration in `mod.rs`**

In `derive/src/accounts/mod.rs` line 7, add:
```rust
mod field_kind;
```

- [ ] **Step 3: Verify build**

Run: `cargo check --manifest-path derive/Cargo.toml`
Expected: clean build (module exists, items are `pub(super)` so no unused warnings)

- [ ] **Step 4: Commit**

```
refactor(derive): add FieldKind enum for centralized type classification
```

---

### Task 2: Migrate `fields.rs` to use FieldKind

**Files:**
- Modify: `derive/src/accounts/fields.rs`

This is the largest task. We replace all ~25 independent classification sites with `FieldKind`-based dispatch. Work section by section.

- [ ] **Step 1: Update imports and delete superseded helpers**

At the top of `fields.rs`, replace the import block and delete these functions entirely:
- `type_base_name` (line 62) — now in `field_kind.rs`
- `is_interface_account_field` (line 113) — replaced by `matches!(kind, FieldKind::InterfaceAccount { .. })`
- `is_token_or_mint_field` (line 124) — replaced by `kind.is_token_or_mint()`
- `is_token_account_field` (line 144) — replaced by `kind.is_token_account()`
- `is_signer_field` (line 163) — replaced by `matches!(kind, FieldKind::Signer)`
- `extract_account_inner_type` (line 261) — replaced by `if let FieldKind::Account { inner_ty } = kind`
- `is_dynamic_account_type` (line 271) — replaced by `kind.is_dynamic()`

Add import:
```rust
use super::field_kind::{strip_ref, type_base_name, FieldKind, FieldFlags};
```

Keep these functions (they scan ALL fields, not a single field):
- `find_field_by_type` — but simplify using `strip_ref` and `type_base_name`
- `find_field_by_name` — unchanged
- `find_field_by_account_inner_type` — simplify using `FieldKind::classify`
- `count_fields_by_type` — simplify using `FieldKind::classify`
- `resolve_token_program_addr` — simplify using `FieldKind::classify`

Simplify `find_field_by_type`:
```rust
fn find_field_by_type<'a>(
    fields: &'a syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    type_names: &[&str],
) -> Option<&'a Ident> {
    for field in fields.iter() {
        let ty = strip_ref(&field.ty);
        if let Some(base) = type_base_name(ty) {
            if type_names.contains(&base.as_str()) {
                return field.ident.as_ref();
            }
        }
        // Check Program<T> / Interface<T> wrappers
        match FieldKind::classify(ty) {
            FieldKind::Program { inner_ty } | FieldKind::Interface { inner_ty } => {
                if let Some(base) = type_base_name(inner_ty) {
                    if type_names.contains(&base.as_str()) {
                        return field.ident.as_ref();
                    }
                }
            }
            _ => {}
        }
    }
    None
}
```

Simplify `count_fields_by_type`:
```rust
fn count_fields_by_type(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    type_names: &[&str],
) -> usize {
    fields
        .iter()
        .filter(|field| {
            let ty = strip_ref(&field.ty);
            match FieldKind::classify(ty) {
                FieldKind::Program { inner_ty } | FieldKind::Interface { inner_ty } => {
                    type_base_name(inner_ty)
                        .as_deref()
                        .is_some_and(|b| type_names.contains(&b))
                }
                _ => false,
            }
        })
        .count()
}
```

Simplify `find_field_by_account_inner_type`:
```rust
fn find_field_by_account_inner_type<'a>(
    fields: &'a syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    inner_type_names: &[&str],
) -> Option<&'a Ident> {
    for field in fields.iter() {
        let ty = strip_ref(&field.ty);
        if let FieldKind::Account { inner_ty } = FieldKind::classify(ty) {
            if let Some(base) = type_base_name(inner_ty) {
                if inner_type_names.contains(&base.as_str()) {
                    return field.ident.as_ref();
                }
            }
        }
    }
    None
}
```

Simplify `resolve_token_program_addr`:
```rust
fn resolve_token_program_addr(
    effective_ty: &Type,
    token_program_field: Option<&Ident>,
) -> proc_macro2::TokenStream {
    let underlying = strip_ref(effective_ty);
    if let FieldKind::Account { inner_ty } = FieldKind::classify(underlying) {
        match type_base_name(inner_ty).as_deref() {
            Some("Token" | "Mint") => return quote! { &quasar_spl::SPL_TOKEN_ID },
            Some("Token2022" | "Mint2022") => return quote! { &quasar_spl::TOKEN_2022_ID },
            _ => {}
        }
    }
    let tp = token_program_field
        .expect("InterfaceAccount with token/ata attrs requires a token program field");
    quote! { #tp.to_account_view().address() }
}
```

- [ ] **Step 2: Update `validate_field_attrs` to take `FieldKind`**

Change signature to take `FieldKind` and `FieldFlags` while keeping `field: &syn::Field` (needed for `#[account(dup)]` doc comment check on `field.attrs`):

```rust
fn validate_field_attrs(
    field: &syn::Field,
    field_name: &Ident,
    attrs: &AccountFieldAttrs,
    kind: &FieldKind,
    flags: &FieldFlags,
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
) -> Result<(), proc_macro::TokenStream> {
```

Replace internal checks:
- `is_writable` → `flags.is_writable` (delete `let is_writable = is_ref_mut || attrs.is_mut;`)
- `is_token_or_mint_field(field)` → `kind.is_token_or_mint()`
- `is_token_account_field(field)` → `kind.is_token_account()`
- Keep `field.attrs` access for `#[account(dup)]` doc check unchanged

The sweep authority Signer check becomes:
```rust
if let Some(auth_name) = &attrs.token_authority {
    let is_auth_signer = fields.iter().any(|f| {
        f.ident.as_ref() == Some(auth_name)
            && matches!(FieldKind::classify(strip_ref(&f.ty)), FieldKind::Signer)
    });
    reject!(
        !is_auth_signer,
        "sweep requires `token::authority` to be a Signer"
    );
}
```

- [ ] **Step 3: Update `DetectedFields::detect` to use FieldKind**

The `rent_sysvar` detection (lines 364-375) becomes:
```rust
let rent_sysvar = fields.iter().find_map(|field| {
    let ty = strip_ref(&field.ty);
    if let FieldKind::Sysvar { inner_ty } = FieldKind::classify(ty) {
        if type_base_name(inner_ty).as_deref() == Some("Rent") {
            return field.ident.as_ref();
        }
    }
    None
});
```

The `has_any_non_init_interface_needing_program` check (lines 708-716) uses `is_interface_account_field` — replace:
```rust
let has_any_non_init_interface_needing_program =
    field_attrs.iter().zip(fields.iter()).any(|(a, f)| {
        let is_init = a.is_init || a.init_if_needed;
        let needs_program = a.token_mint.is_some()
            || (a.associated_token_mint.is_some()
                && a.associated_token_token_program.is_none())
            || a.mint_decimals.is_some();
        let ty = strip_ref(&f.ty);
        !is_init && needs_program && matches!(FieldKind::classify(ty), FieldKind::InterfaceAccount { .. })
    });
```

The `has_any_token_close` check (lines 697-700):
```rust
let has_any_token_close = field_attrs
    .iter()
    .zip(fields.iter())
    .any(|(a, f)| {
        let ty = strip_ref(&f.ty);
        a.close.is_some() && FieldKind::classify(ty).is_token_or_mint()
    });
```

- [ ] **Step 4: Rewrite the main loop to classify once and use FieldKind**

At the start of the main loop (line 831), add classification:
```rust
for (field, attrs) in fields.iter().zip(field_attrs.iter()) {
    let field_name = field.ident.as_ref().unwrap();

    let is_optional = extract_generic_inner_type(&field.ty, "Option").is_some();
    let effective_ty = extract_generic_inner_type(&field.ty, "Option").unwrap_or(&field.ty);
    let is_ref_mut = matches!(effective_ty, Type::Reference(r) if r.mutability.is_some());
    let underlying_ty = strip_ref(effective_ty);
    let kind = FieldKind::classify(underlying_ty);
    let flags = FieldFlags::compute(&kind, attrs, is_ref_mut);

    validate_field_attrs(field, field_name, attrs, &kind, &flags, fields)?;
```

- [ ] **Step 5: Replace the validation if-else chain (~lines 868-971) with match on kind**

All 32-byte address comparisons use `keys_eq` (word-wise u64 short-circuit, ~2 CU on SBF when first 8 bytes differ) instead of `PartialEq`/`memcmp` (~8 CU). The `debug_checked` helper DRYs the repeated 8-line debug/non-debug pattern.

```rust
    use super::field_kind::debug_checked;

    // Generate type-specific validation (owner, discriminator, address).
    let has_inline_validation = attrs.token_mint.is_some()
        || attrs.associated_token_mint.is_some()
        || attrs.mint_decimals.is_some();
    let skip_mut_checks = (attrs.is_init || attrs.init_if_needed) && has_inline_validation;

    let field_name_str = field_name.to_string();

    if !skip_mut_checks {
        let validation = match &kind {
            FieldKind::Account { inner_ty } => {
                let owner = debug_checked(
                    &field_name_str,
                    quote! { <#inner_ty as quasar_lang::traits::CheckOwner>::check_owner(#field_name.to_account_view()) },
                    "Owner check failed for account '{}'",
                );
                let disc = debug_checked(
                    &field_name_str,
                    quote! { <#inner_ty as quasar_lang::traits::AccountCheck>::check(#field_name.to_account_view()) },
                    "Discriminator check failed for account '{}': data may be uninitialized or corrupted",
                );
                Some(quote! { #owner #disc })
            }
            FieldKind::InterfaceAccount { inner_ty } => {
                let disc = debug_checked(
                    &field_name_str,
                    quote! { <#inner_ty as quasar_lang::traits::AccountCheck>::check(#field_name.to_account_view()) },
                    "Account check failed for interface account '{}': data may be uninitialized or corrupted",
                );
                Some(quote! {
                    {
                        let __owner = #field_name.to_account_view().owner();
                        if quasar_lang::utils::hint::unlikely(
                            !quasar_lang::keys_eq(__owner, &quasar_spl::SPL_TOKEN_ID)
                                && !quasar_lang::keys_eq(__owner, &quasar_spl::TOKEN_2022_ID)
                        ) {
                            #[cfg(feature = "debug")]
                            quasar_lang::prelude::log(&::alloc::format!(
                                "Owner check failed for interface account '{}': not owned by SPL Token or Token-2022",
                                #field_name_str
                            ));
                            return Err(ProgramError::IllegalOwner);
                        }
                    }
                    #disc
                })
            }
            // keys_eq: word-wise u64 comparison, short-circuits on first mismatch
            FieldKind::Sysvar { inner_ty } => Some(quote! {
                if quasar_lang::utils::hint::unlikely(
                    !quasar_lang::keys_eq(#field_name.to_account_view().address(), &<#inner_ty as quasar_lang::sysvars::Sysvar>::ID)
                ) {
                    #[cfg(feature = "debug")]
                    quasar_lang::prelude::log(&::alloc::format!(
                        "Incorrect sysvar address for account '{}': expected {}, got {}",
                        #field_name_str,
                        <#inner_ty as quasar_lang::sysvars::Sysvar>::ID,
                        #field_name.to_account_view().address()
                    ));
                    return Err(ProgramError::IncorrectProgramId);
                }
            }),
            // keys_eq: word-wise u64 comparison, short-circuits on first mismatch
            FieldKind::Program { inner_ty } => Some(quote! {
                if quasar_lang::utils::hint::unlikely(
                    !quasar_lang::keys_eq(#field_name.to_account_view().address(), &<#inner_ty as quasar_lang::traits::Id>::ID)
                ) {
                    #[cfg(feature = "debug")]
                    quasar_lang::prelude::log(&::alloc::format!(
                        "Incorrect program ID for account '{}': expected {}, got {}",
                        #field_name_str,
                        <#inner_ty as quasar_lang::traits::Id>::ID,
                        #field_name.to_account_view().address()
                    ));
                    return Err(ProgramError::IncorrectProgramId);
                }
            }),
            FieldKind::Interface { inner_ty } => Some(quote! {
                if quasar_lang::utils::hint::unlikely(
                    !<#inner_ty as quasar_lang::traits::ProgramInterface>::matches(#field_name.to_account_view().address())
                ) {
                    #[cfg(feature = "debug")]
                    quasar_lang::prelude::log(&::alloc::format!(
                        "Program interface mismatch for account '{}': address {} does not match any allowed programs",
                        #field_name_str,
                        #field_name.to_account_view().address()
                    ));
                    return Err(ProgramError::IncorrectProgramId);
                }
            }),
            FieldKind::SystemAccount => {
                let base_type = strip_generics(underlying_ty);
                Some(debug_checked(
                    &field_name_str,
                    quote! { <#base_type as quasar_lang::checks::Owner>::check(#field_name.to_account_view()) },
                    "Owner check failed for account '{}': not owned by system program",
                ))
            }
            FieldKind::Signer | FieldKind::Other => None,
        };

        if let Some(check) = validation {
            // Grouped per-field: pushed to per_field_checks, wrapped in Step 5b
            per_field_checks.push((field_name.clone(), is_optional, check));
        }
    }
```

- [ ] **Step 5b: Restructure check vectors — group optional checks per-field**

Currently, checks for a single optional field are scattered across 4 vectors (`mut_checks`, `has_one_checks`, `constraint_checks`, `pda_checks`), each independently wrapped in `if let Some(ref field) = field`. This generates N separate `if let Some` branches for an optional field with N check types.

**Replace** the 4 separate vectors with a single per-field accumulator:

```rust
// Before (in ProcessedFields):
pub has_one_checks: Vec<proc_macro2::TokenStream>,
pub constraint_checks: Vec<proc_macro2::TokenStream>,
pub mut_checks: Vec<proc_macro2::TokenStream>,
pub pda_checks: Vec<proc_macro2::TokenStream>,

// After:
pub field_checks: Vec<proc_macro2::TokenStream>,
```

In the main loop, accumulate ALL checks for each field into a local `Vec`, then at the end of the per-field iteration, emit a single grouped block:

```rust
    // At the start of each field's iteration:
    let mut this_field_checks: Vec<proc_macro2::TokenStream> = Vec::new();

    // Validation checks (from Step 5) push to this_field_checks
    // has_one checks push to this_field_checks
    // constraint checks push to this_field_checks
    // address checks push to this_field_checks
    // pda checks push to this_field_checks

    // At the end of each field's iteration — emit ONE block:
    if !this_field_checks.is_empty() {
        if is_optional {
            field_checks.push(quote! {
                if let Some(ref #field_name) = #field_name {
                    #(#this_field_checks)*
                }
            });
        } else {
            field_checks.extend(this_field_checks);
        }
    }
```

Update `mod.rs` parse body generation (lines 359-475) to use the new single vector:

1. Replace `has_any_checks` (line 361-366):
```rust
// Before:
let has_any_checks = !pf.has_one_checks.is_empty()
    || !pf.constraint_checks.is_empty()
    || !pf.mut_checks.is_empty()
    || !pf.pda_checks.is_empty()
    || !pf.init_pda_checks.is_empty()
    || !pf.init_blocks.is_empty();

// After:
let has_any_checks = !pf.field_checks.is_empty()
    || !pf.init_pda_checks.is_empty()
    || !pf.init_blocks.is_empty();
```

2. Replace the 4 vector bindings (lines 370-373):
```rust
// Before:
let mut_checks = &pf.mut_checks;
let has_one_checks = &pf.has_one_checks;
let constraint_checks = &pf.constraint_checks;
let pda_checks = &pf.pda_checks;

// After:
let field_checks = &pf.field_checks;
```

3. In ALL THREE parse_body branches (lines 419-422, 457-460), replace:
```rust
// Before:
#(#mut_checks)*
#(#has_one_checks)*
#(#constraint_checks)*
#(#pda_checks)*

// After:
#(#field_checks)*
```

This collapses N `if let Some` branches into 1 per optional field.

- [ ] **Step 5c: Use `keys_eq` for has_one and address checks**

Replace the has_one comparison (currently `!=` which uses memcmp):
```rust
    for (target, custom_error) in &attrs.has_ones {
        let error = match custom_error {
            Some(err) => quote! { #err.into() },
            None => quote! { QuasarError::HasOneMismatch.into() },
        };
        // keys_eq: word-wise u64 comparison, ~2 CU when first 8 bytes differ
        this_field_checks.push(quote! {
            if !quasar_lang::keys_eq(&#field_name.#target, #target.to_account_view().address()) {
                return Err(#error);
            }
        });
    }
```

Replace the address comparison:
```rust
    if let Some((addr_expr, custom_error)) = &attrs.address {
        let error = match custom_error {
            Some(err) => quote! { #err.into() },
            None => quote! { QuasarError::AddressMismatch.into() },
        };
        // keys_eq: word-wise u64 comparison
        this_field_checks.push(quote! {
            if !quasar_lang::keys_eq(#field_name.to_account_view().address(), &#addr_expr) {
                return Err(#error);
            }
        });
    }
```

- [ ] **Step 5d: Add BUMP_OFFSET bounds check in PDA validation codegen**

**Security fix:** The PDA validation path (fields.rs ~line 1341) reads `unsafe { *view.data_ptr().add(__offset) }` without verifying that `__offset < data_len`. If `BUMP_OFFSET` points beyond the account's data (e.g. from a misconfigured or attacker-controlled type), this is an out-of-bounds read.

In the PDA codegen block that uses `BUMP_OFFSET` (the `if let Some(__offset) = <#inner_ty as Discriminator>::BUMP_OFFSET` branch), add a bounds check before the unsafe dereference:

```rust
if let Some(__offset) = <#inner_ty as Discriminator>::BUMP_OFFSET {
    if quasar_lang::utils::hint::unlikely(__offset >= #view_access.data_len()) {
        #[cfg(feature = "debug")]
        quasar_lang::prelude::log(&::alloc::format!(
            "BUMP_OFFSET {} out of bounds for account '{}' (data_len={})",
            __offset, #field_name_str, #view_access.data_len()
        ));
        return Err(ProgramError::AccountDataTooSmall);
    }
    let __bump_val: u8 = unsafe { *#view_access.data_ptr().add(__offset) };
    // ... rest unchanged
}
```

This is a compile-time constant offset so the check is almost always branch-predicted away, but it defends against corrupted or truncated account data.

- [ ] **Step 6: Replace field construction to use FieldKind**

Replace the `match effective_ty` block (~lines 982-1010):
```rust
    let construct = |expr: proc_macro2::TokenStream| {
        if is_optional {
            quote! { #field_name: if quasar_lang::keys_eq(#field_name.address(), __program_id) { None } else { Some(#expr) } }
        } else {
            quote! { #field_name: #expr }
        }
    };

    if kind.is_dynamic() {
        if let FieldKind::Account { inner_ty } = &kind {
            let inner_base = strip_generics(inner_ty);
            field_constructs.push(construct(
                quote! { #inner_base::from_account_view(#field_name)? },
            ));
        } else {
            let base_type = strip_generics(effective_ty);
            field_constructs.push(quote! { #field_name: #base_type::from_account_view(#field_name)? });
        }
    } else if let Type::Reference(type_ref) = effective_ty {
        let base_type = strip_generics(&type_ref.elem);
        let expr = if type_ref.mutability.is_some() {
            quote! { unsafe { #base_type::from_account_view_unchecked_mut(#field_name) } }
        } else {
            quote! { unsafe { #base_type::from_account_view_unchecked(#field_name) } }
        };
        field_constructs.push(construct(expr));
    } else {
        let base_type = strip_generics(effective_ty);
        field_constructs.push(construct(
            quote! { unsafe { #base_type::from_account_view_unchecked(#field_name) } },
        ));
    }
```

- [ ] **Step 7: Replace close/sweep type checks**

Line 1069 `is_token_or_mint_field(field)` → `kind.is_token_or_mint()`

Line 1126 `is_token_account_field(receiver_field)` → `FieldKind::classify(strip_ref(&receiver_field.ty)).is_token_account()`

Line 1153 `is_signer_field(af)` → `matches!(FieldKind::classify(strip_ref(&af.ty)), FieldKind::Signer)`

- [ ] **Step 8: Replace `determine_nodup_constant` and `compute_header_expected`**

Replace both functions at the bottom of `fields.rs`.

Note: `determine_nodup_constant` intentionally does NOT strip `Option<>` because it's only called from the no-dup path (mod.rs line 235), which excludes optional fields. `compute_header_expected` DOES strip `Option<>` because it's called from the dup-aware path which handles optionals.

```rust
pub(super) fn determine_nodup_constant(
    field: &syn::Field,
    attrs: &super::attrs::AccountFieldAttrs,
    is_ref_mut: bool,
) -> &'static str {
    // No Option stripping — this is only called for non-optional, non-dup fields.
    let ty = strip_ref(&field.ty);
    let kind = FieldKind::classify(ty);
    FieldFlags::compute(&kind, attrs, is_ref_mut).nodup_constant()
}

pub(super) fn compute_header_expected(
    field: &syn::Field,
    attrs: &super::attrs::AccountFieldAttrs,
    is_ref_mut: bool,
) -> u32 {
    let effective_ty = extract_generic_inner_type(&field.ty, "Option").unwrap_or(&field.ty);
    let ty = strip_ref(effective_ty);
    let kind = FieldKind::classify(ty);
    FieldFlags::compute(&kind, attrs, is_ref_mut).header_constant()
}
```

- [ ] **Step 9: Verify build**

Run: `cargo check`
Expected: clean build. All 25+ classification sites now go through FieldKind.

- [ ] **Step 10: Commit**

```
refactor(derive): migrate to FieldKind enum — classify once, match everywhere

- FieldKind enum replaces ~25 stringly-typed classification sites
- FieldFlags precomputes signer/writable/executable from kind + attrs
- keys_eq for all 32-byte comparisons (has_one, address, Program, Sysvar)
- Grouped optional checks: single if-let-Some per field, not per category
- debug_checked helper DRYs the debug/non-debug check pattern
```

---

### Task 3: Restructure Optional account validation — sentinel-first (fixes #101)

**Files:**
- Modify: `derive/src/accounts/mod.rs` (dup-aware path + parse_accounts signature)
- Modify: `lang/src/entrypoint.rs:46` (pass program_id to parse_accounts)

**Root cause of #101:** The code special-cases individual checks for optional accounts (blanking `exec_check`). The fix: determine Some/None via the sentinel check (address == program_id) FIRST, then apply ALL standard checks for present accounts, skip ALL for absent. No per-check special-casing.

- [ ] **Step 1: Update `parse_accounts` generated signature (mod.rs ~line 627)**

```rust
// Before:
pub unsafe fn parse_accounts(
    mut input: *mut u8,
    buf: &mut core::mem::MaybeUninit<[quasar_lang::__internal::AccountView; #count_expr]>,
) -> Result<*mut u8, ProgramError> {

// After:
pub unsafe fn parse_accounts(
    mut input: *mut u8,
    buf: &mut core::mem::MaybeUninit<[quasar_lang::__internal::AccountView; #count_expr]>,
    __program_id: &[u8; 32],
) -> Result<*mut u8, ProgramError> {
```

- [ ] **Step 2: Update composite type calls (mod.rs ~line 128)**

```rust
// Before:
input = <#inner_ty>::parse_accounts(input, &mut __inner_buf)?;

// After:
input = <#inner_ty>::parse_accounts(input, &mut __inner_buf, __program_id)?;
```

- [ ] **Step 3: Update entrypoint dispatch macro (entrypoint.rs ~line 46)**

```rust
// Before:
<$accounts_ty>::parse_accounts(__accounts_start, &mut __buf)?

// After:
<$accounts_ty>::parse_accounts(__accounts_start, &mut __buf, __program_id)?
```

- [ ] **Step 4: Restructure the dup-aware path (mod.rs ~lines 161-232)**

Import `FieldFlags` at the top of the dup-aware path usage. Replace the per-flag `flag_check` + `exec_check` generation with a **single masked u32 comparison** — same approach as the no-dup path, just with a sentinel guard for optional accounts:

```rust
if is_optional || attrs.dup {
    let effective_ty = extract_generic_inner_type(&field.ty, "Option").unwrap_or(&field.ty);
    let is_ref_mut = matches!(effective_ty, Type::Reference(r) if r.mutability.is_some());
    // Use FieldFlags from field_kind — single source of truth
    let expected_header = fields::compute_header_expected(field, attrs, is_ref_mut);

    // Single masked comparison: mask out borrow_state (byte 0), compare all flags at once.
    // Replaces 2-3 separate shift+mask+compare operations with 1 AND + 1 compare.
    use super::field_kind::FLAG_MASK;
    let flag_mask: u32 = FLAG_MASK;
    let expected_masked = expected_header & flag_mask;
    let flag_check = quote! {
        if quasar_lang::utils::hint::unlikely((actual_header & #flag_mask) != #expected_masked) {
            #[cfg(feature = "debug")]
            quasar_lang::__internal::log_str(concat!(
                "Account '", stringify!(#field_name),
                "' (index ", #account_index, "): header flags mismatch"
            ));
            return Err(ProgramError::from(quasar_lang::decode_header_error(actual_header, #expected_header)));
        }
    };

    // For optional: sentinel guard wraps ALL checks.
    // Use keys_eq for consistency — word-wise u64 comparison.
    let guarded_checks = if is_optional {
        quote! {
            if !quasar_lang::keys_eq(unsafe { &(*raw).address }, __program_id) {
                #flag_check
            }
        }
    } else {
        flag_check
    };

    parse_steps.push(quote! {
        {
            let raw = input as *mut quasar_lang::__internal::RuntimeAccount;
            let actual_header = unsafe { *(raw as *const u32) };

            if (actual_header & 0xFF) == quasar_lang::__internal::NOT_BORROWED as u32 {
                #guarded_checks
                unsafe {
                    core::ptr::write(base.add(#cur_offset), quasar_lang::__internal::AccountView::new_unchecked(raw));
                    input = input.add(__ACCOUNT_HEADER.wrapping_add((*raw).data_len as usize));
                    input = input.add((input as usize).wrapping_neg() & 7);
                }
            } else {
                // Security: bounds-check the dup index before using it to read
                // from the AccountView buffer. Without this, a malicious dup byte
                // could cause an out-of-bounds read from the buf array.
                let idx = (actual_header & 0xFF) as usize;
                if quasar_lang::utils::hint::unlikely(idx >= #cur_offset) {
                    return Err(ProgramError::InvalidAccountData);
                }
                unsafe {
                    core::ptr::write(base.add(#cur_offset), core::ptr::read(base.add(idx)));
                    input = input.add(core::mem::size_of::<u64>());
                }
            }
        }
    });
}
```

- [ ] **Step 5: Add `checked_mul` for instruction arg Vec parsing (mod.rs ~line 839)**

**Security fix:** The instruction arg parsing in `mod.rs` line 839 does `__ix_dyn_count * core::mem::size_of::<#elem>()` where `__ix_dyn_count` comes from instruction data. If an attacker provides a huge count, this can overflow `usize` on 32-bit (SBF is 64-bit but defense-in-depth applies), causing the subsequent bounds check to pass with a wrapped-around value.

Replace:
```rust
let __ix_dyn_byte_len = __ix_dyn_count * core::mem::size_of::<#elem>();
```

With:
```rust
let __ix_dyn_byte_len = __ix_dyn_count
    .checked_mul(core::mem::size_of::<#elem>())
    .ok_or(ProgramError::InvalidInstructionData)?;
```

- [ ] **Step 6: Verify build**

Run: `cargo check`
Expected: clean build

- [ ] **Step 7: Commit**

```
fix(derive): restructure Optional account validation — sentinel-first

Instead of special-casing individual checks (flag, executable) for
optional accounts, determine Some/None via the sentinel check first,
then apply ALL standard checks when the account is present. Dup-aware
path uses single masked u32 comparison instead of 2-3 separate flag
checks, dup index is bounds-checked, and instruction arg Vec parsing
uses checked_mul. This prevents the class of bugs where new checks
forget to handle optionals and hardens against malicious input.

Closes #101
```

---

### Task 4: Extract init codegen into `init.rs` with shared CPI body

**Files:**
- Create: `derive/src/accounts/init.rs`
- Modify: `derive/src/accounts/fields.rs` (~lines 1441-1795 move out)
- Modify: `derive/src/accounts/mod.rs` (add `mod init;`)

Three things happen here: (1) extract init codegen into its own file, (2) deduplicate the init CPI body (token/mint/Account<T> all share `try_minimum_balance` + `init_account`), (3) fix `needs_rent` by tracking which init paths actually use `__shared_rent`.

- [ ] **Step 1: Create `init.rs` with InitContext and shared helpers**

```rust
//! Init codegen for `#[derive(Accounts)]`.
//!
//! Generates CPI calls for account initialization: token accounts, mints,
//! generic Account<T>, ATAs, metadata, and master editions.

use {
    super::{
        attrs::AccountFieldAttrs,
        field_kind::{strip_ref, FieldKind},
    },
    crate::helpers::{extract_generic_inner_type, seed_slice_expr_for_parse, strip_generics},
    quote::{format_ident, quote},
    syn::{Ident, Type},
};

/// Context needed by init codegen, gathered from DetectedFields + per-field locals.
pub(super) struct InitContext<'a> {
    pub payer: &'a Ident,
    pub system_program: &'a Ident,
    pub token_program: Option<&'a Ident>,
    pub ata_program: Option<&'a Ident>,
    pub metadata_account: Option<&'a Ident>,
    pub master_edition_account: Option<&'a Ident>,
    pub metadata_program: Option<&'a Ident>,
    pub mint_authority: Option<&'a Ident>,
    pub update_authority: Option<&'a Ident>,
    pub rent: Option<&'a Ident>,
    pub field_name_strings: &'a [String],
}

/// Result of generating an init block.
pub(super) struct InitBlockResult {
    pub tokens: proc_macro2::TokenStream,
    /// True if this init path uses `__shared_rent` (false for ATA, metadata, master edition).
    pub uses_rent: bool,
}

/// Build PDA signer seeds setup and reference for init_account calls.
fn gen_signers(
    field_name: &Ident,
    attrs: &AccountFieldAttrs,
    field_name_strings: &[String],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    if let Some(seed_exprs) = &attrs.seeds {
        let bump_var = format_ident!("__bumps_{}", field_name);
        let seed_slices: Vec<proc_macro2::TokenStream> = seed_exprs
            .iter()
            .map(|expr| seed_slice_expr_for_parse(expr, field_name_strings))
            .collect();
        (
            quote! {
                let __init_bump_ref: &[u8] = &[#bump_var];
                let __init_signer_seeds = [#(quasar_lang::cpi::Seed::from(#seed_slices),)* quasar_lang::cpi::Seed::from(__init_bump_ref)];
                let __init_signers = [quasar_lang::cpi::Signer::from(&__init_signer_seeds[..])];
            },
            quote! { &__init_signers },
        )
    } else {
        (quote! {}, quote! { &[] })
    }
}

/// Shared init CPI body: try_minimum_balance + init_account + post_init.
/// Used by token, mint, and Account<T> init (NOT ATA — ATA uses its own CPI).
fn gen_init_cpi_body(
    pay_field: &Ident,
    field_name: &Ident,
    space_expr: proc_macro2::TokenStream,
    owner_expr: proc_macro2::TokenStream,
    signers_setup: &proc_macro2::TokenStream,
    signers_ref: &proc_macro2::TokenStream,
    post_init: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        let __init_lamports = __shared_rent.try_minimum_balance(#space_expr as usize)?;
        #signers_setup
        quasar_lang::cpi::system::init_account(
            #pay_field, #field_name, __init_lamports, #space_expr as u64,
            #owner_expr, #signers_ref,
        )?;
        #post_init
    }
}

/// Wrap a CPI body with the init/init_if_needed guard pattern.
pub(super) fn wrap_init_block(
    field_name: &Ident,
    init_if_needed: bool,
    cpi_body: proc_macro2::TokenStream,
    validate_existing: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    if init_if_needed {
        let validate = validate_existing.unwrap_or_default();
        quote! {
            {
                if quasar_lang::is_system_program(#field_name.owner()) {
                    #cpi_body
                } else {
                    #validate
                }
            }
        }
    } else {
        quote! {
            {
                if !quasar_lang::is_system_program(#field_name.owner()) {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
                #cpi_body
            }
        }
    }
}

/// Generate the init block for a field. Returns None if not an init field
/// or if the field type doesn't support init.
pub(super) fn gen_init_block(
    field_name: &Ident,
    attrs: &AccountFieldAttrs,
    effective_ty: &Type,
    ctx: &InitContext,
) -> Result<Option<InitBlockResult>, proc_macro::TokenStream> {
    if !attrs.is_init && !attrs.init_if_needed {
        return Ok(None);
    }

    let (signers_setup, signers_ref) = gen_signers(field_name, attrs, ctx.field_name_strings);

    // --- ATA init ---
    if attrs.associated_token_mint.is_some() {
        let ata_prog = ctx.ata_program.unwrap();
        let mint_field = attrs.associated_token_mint.as_ref().unwrap();
        let auth_field = attrs.associated_token_authority.as_ref().unwrap();
        let sys_field = ctx.system_program;
        let tok_field = attrs
            .associated_token_token_program
            .as_ref()
            .unwrap_or_else(|| ctx.token_program.unwrap());
        let token_program_addr = if let Some(tp) = &attrs.associated_token_token_program {
            quote! { #tp.address() }
        } else {
            let tp = ctx.token_program.unwrap();
            quote! { #tp.address() }
        };

        let ata_cpi = |instruction_byte: u8| {
            quote! {
                quasar_lang::cpi::CpiCall::new(
                    #ata_prog.address(),
                    [
                        quasar_lang::cpi::InstructionAccount::writable_signer(#ctx.payer.address()),
                        quasar_lang::cpi::InstructionAccount::writable(#field_name.address()),
                        quasar_lang::cpi::InstructionAccount::readonly(#auth_field.address()),
                        quasar_lang::cpi::InstructionAccount::readonly(#mint_field.address()),
                        quasar_lang::cpi::InstructionAccount::readonly(#sys_field.address()),
                        quasar_lang::cpi::InstructionAccount::readonly(#tok_field.address()),
                    ],
                    [#ctx.payer, #field_name, #auth_field, #mint_field, #sys_field, #tok_field],
                    [#instruction_byte],
                ).invoke()?;
            }
        };

        let validate = quote! {
            quasar_spl::validate_ata(
                #field_name.to_account_view(),
                #auth_field.to_account_view().address(),
                #mint_field.to_account_view().address(),
                #token_program_addr,
            )?;
        };

        let block = wrap_init_block(
            field_name,
            attrs.init_if_needed,
            ata_cpi(if attrs.init_if_needed { 1 } else { 0 }),
            Some(validate),
        );
        return Ok(Some(InitBlockResult {
            tokens: block,
            uses_rent: false, // ATA program handles rent
        }));
    }

    // --- Token init ---
    if attrs.token_mint.is_some() {
        let tok_field = ctx.token_program.unwrap();
        let mint_field = attrs.token_mint.as_ref().unwrap();
        let auth_field = attrs.token_authority.as_ref().unwrap();
        let pay = ctx.payer;

        let cpi_body = gen_init_cpi_body(
            pay,
            field_name,
            quote! { quasar_spl::TokenAccountState::LEN },
            quote! { #tok_field.address() },
            &signers_setup,
            &signers_ref,
            quote! {
                quasar_spl::initialize_account3(
                    #tok_field, #field_name, #mint_field, #auth_field.address(),
                ).invoke()?;
            },
        );
        let tok_addr = quote! { #tok_field.address() };
        let validate = quote! {
            quasar_spl::validate_token_account(
                #field_name.to_account_view(),
                #mint_field.to_account_view().address(),
                #auth_field.to_account_view().address(),
                #tok_addr,
            )?;
        };
        let block = wrap_init_block(field_name, attrs.init_if_needed, cpi_body, Some(validate));
        return Ok(Some(InitBlockResult {
            tokens: block,
            uses_rent: true,
        }));
    }

    // --- Mint init ---
    if let Some(decimals_expr) = attrs.mint_decimals.as_ref() {
        let tok_field = ctx.token_program.unwrap();
        let auth_field = attrs.mint_init_authority.as_ref().ok_or_else(|| {
            syn::Error::new_spanned(field_name, "`mint::decimals` requires `mint::authority = <field>`")
                .to_compile_error()
                .into()
        })?;
        let freeze_expr = if let Some(ff) = &attrs.mint_freeze_authority {
            quote! { Some(#ff.address()) }
        } else {
            quote! { None }
        };
        let pay = ctx.payer;

        let cpi_body = gen_init_cpi_body(
            pay,
            field_name,
            quote! { quasar_spl::MintAccountState::LEN },
            quote! { #tok_field.address() },
            &signers_setup,
            &signers_ref,
            quote! {
                quasar_spl::initialize_mint2(
                    #tok_field, #field_name,
                    (#decimals_expr) as u8,
                    #auth_field.address(),
                    #freeze_expr,
                ).invoke()?;
            },
        );
        let tok_addr = quote! { #tok_field.address() };
        let freeze_validate = if let Some(ff) = &attrs.mint_freeze_authority {
            quote! { Some(#ff.to_account_view().address()) }
        } else {
            quote! { None }
        };
        let validate = quote! {
            quasar_spl::validate_mint(
                #field_name.to_account_view(),
                #auth_field.to_account_view().address(),
                (#decimals_expr) as u8,
                #freeze_validate,
                #tok_addr,
            )?;
        };
        let block = wrap_init_block(field_name, attrs.init_if_needed, cpi_body, Some(validate));
        return Ok(Some(InitBlockResult {
            tokens: block,
            uses_rent: true,
        }));
    }

    // --- Generic Account<T> init ---
    let underlying = strip_ref(effective_ty);
    let inner_type = if let FieldKind::Account { inner_ty } = FieldKind::classify(underlying) {
        strip_generics(inner_ty)
    } else {
        return Err(syn::Error::new_spanned(
            field_name,
            "#[account(init)] on non-Account<T> type requires `token::mint` and \
             `token::authority`, `associated_token::mint` and \
             `associated_token::authority`, or `mint::decimals` and `mint::authority`",
        )
        .to_compile_error()
        .into());
    };

    let space_expr = if let Some(space) = &attrs.space {
        quote! { (#space) as u64 }
    } else {
        quote! { <#inner_type as quasar_lang::traits::Space>::SPACE as u64 }
    };

    // Security: verify allocated space can hold the discriminator before writing.
    // Without this, a user-provided `space` value smaller than the discriminator
    // length would cause an out-of-bounds write via copy_nonoverlapping.
    let cpi_body = gen_init_cpi_body(
        ctx.payer,
        field_name,
        space_expr.clone(),
        quote! { &crate::ID },
        &signers_setup,
        &signers_ref,
        quote! {
            let __disc = <#inner_type as quasar_lang::traits::Discriminator>::DISCRIMINATOR;
            if quasar_lang::utils::hint::unlikely((#space_expr as usize) < __disc.len()) {
                return Err(ProgramError::AccountDataTooSmall);
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    __disc.as_ptr(), #field_name.data_mut_ptr(), __disc.len(),
                );
            }
        },
    );
    let block = wrap_init_block(field_name, attrs.init_if_needed, cpi_body, None);
    Ok(Some(InitBlockResult {
        tokens: block,
        uses_rent: true,
    }))
}

/// Generate metadata CPI init block. Returns None if no metadata attrs.
pub(super) fn gen_metadata_init(
    field_name: &Ident,
    attrs: &AccountFieldAttrs,
    ctx: &InitContext,
) -> Option<proc_macro2::TokenStream> {
    let meta_name = attrs.metadata_name.as_ref()?;
    let meta_symbol = attrs.metadata_symbol.as_ref().unwrap();
    let meta_uri = attrs.metadata_uri.as_ref().unwrap();
    let seller_fee = attrs
        .metadata_seller_fee_basis_points
        .as_ref()
        .map(|e| quote! { (#e) as u16 })
        .unwrap_or(quote! { 0u16 });
    let is_mutable = attrs
        .metadata_is_mutable
        .as_ref()
        .map(|e| quote! { #e })
        .unwrap_or(quote! { false });

    let meta_field = ctx.metadata_account.unwrap();
    let meta_prog = ctx.metadata_program.unwrap();
    let mint_auth = ctx.mint_authority.unwrap();
    let update_auth = ctx.update_authority.unwrap();
    let pay = ctx.payer;
    let sys = ctx.system_program;
    let rent = ctx.rent.unwrap();

    Some(quote! {
        {
            quasar_spl::metadata::MetadataCpi::create_metadata_accounts_v3(
                #meta_prog, #meta_field, #field_name, #mint_auth,
                #pay, #update_auth, #sys, #rent,
                quasar_lang::borsh::BorshString::new(#meta_name),
                quasar_lang::borsh::BorshString::new(#meta_symbol),
                quasar_lang::borsh::BorshString::new(#meta_uri),
                #seller_fee, #is_mutable, true,
            ).invoke()?;
        }
    })
}

/// Generate master edition CPI init block. Returns None if no master_edition attrs.
pub(super) fn gen_master_edition_init(
    field_name: &Ident,
    attrs: &AccountFieldAttrs,
    ctx: &InitContext,
) -> Option<proc_macro2::TokenStream> {
    let max_supply = attrs.master_edition_max_supply.as_ref()?;

    let me_field = ctx.master_edition_account.unwrap();
    let meta_field = ctx.metadata_account.unwrap();
    let meta_prog = ctx.metadata_program.unwrap();
    let mint_auth = ctx.mint_authority.unwrap();
    let update_auth = ctx.update_authority.unwrap();
    let pay = ctx.payer;
    let tok = ctx.token_program.unwrap();
    let sys = ctx.system_program;
    let rent = ctx.rent.unwrap();

    Some(quote! {
        {
            quasar_spl::metadata::MetadataCpi::create_master_edition_v3(
                #meta_prog, #me_field, #field_name, #update_auth,
                #mint_auth, #pay, #meta_field, #tok, #sys, #rent,
                Some(#max_supply as u64),
            ).invoke()?;
        }
    })
}
```

**⚠️ IMPORTANT:** The `ata_cpi` closure above uses `#ctx.payer` which does NOT work in `quote!` — you cannot dot-access on interpolated variables. You MUST bind all ctx fields to local variables first, like the token/mint paths do:
```rust
let pay = ctx.payer;
let ata_prog = ctx.ata_program.unwrap();
// ... then use #pay, #ata_prog in quote!
```
The code above is pseudocode showing the logic. During implementation, replace every `#ctx.field` with a pre-bound local variable.

- [ ] **Step 2: Replace init codegen in `fields.rs` main loop**

Replace lines ~1441-1795 (the entire init section + metadata + master edition) with:

```rust
    // --- Init code generation ---
    if attrs.is_init || attrs.init_if_needed {
        let init_ctx = init::InitContext {
            payer: payer_field.unwrap(),
            system_program: system_program_field.unwrap(),
            token_program: token_program_field,
            ata_program: ata_program_field,
            metadata_account: metadata_account_field,
            master_edition_account: master_edition_account_field,
            metadata_program: metadata_program_field,
            mint_authority: mint_authority_field,
            update_authority: update_authority_field,
            rent: rent_field,
            field_name_strings,
        };

        if let Some(result) = init::gen_init_block(field_name, attrs, effective_ty, &init_ctx)? {
            init_blocks.push(result.tokens);
            needs_rent |= result.uses_rent;
        }

        // Metadata CPI (does not use __shared_rent)
        if let Some(block) = init::gen_metadata_init(field_name, attrs, &init_ctx) {
            init_blocks.push(block);
        }

        // Master edition CPI (does not use __shared_rent)
        if let Some(block) = init::gen_master_edition_init(field_name, attrs, &init_ctx) {
            init_blocks.push(block);
        }
    }
```

- [ ] **Step 3: Fix `needs_rent` tracking**

Replace line 1798 (`let needs_rent = !init_blocks.is_empty();`) with declaration before the loop:
```rust
let mut needs_rent = false;
```

Also set `needs_rent = true` for realloc (line ~1720):
```rust
if let Some(realloc_expr) = &attrs.realloc {
    needs_rent = true;
    // ... existing realloc block push ...
}
```

- [ ] **Step 4: Rename `_system_program_field` → `system_program_field`**

Line 642: remove the underscore prefix. Now used by InitContext.

- [ ] **Step 5: Move `wrap_init_block` out of `fields.rs`**

Delete `wrap_init_block` from fields.rs (lines 403-435) — it's now in `init.rs`.

- [ ] **Step 6: Add `mod init;` in `mod.rs`**

- [ ] **Step 7: Verify build**

Run: `cargo check`
Expected: clean build

- [ ] **Step 8: Commit**

```
refactor(derive): extract init codegen into init.rs, fix needs_rent tracking

- Shared gen_init_cpi_body deduplicates token/mint/Account<T> init CPI
- InitContext replaces 16 positional params
- needs_rent tracks per-path (false for ATA, metadata, master edition)
- Realloc now correctly sets needs_rent
```

---

### Task 5: Clean up `lang/` runtime — system.rs + account.rs

**Files:**
- Modify: `lang/src/cpi/system.rs`
- Modify: `lang/src/accounts/account.rs`

- [ ] **Step 1: Add `init_account` method to `impl Program<System>`**

After the existing `assign` method (~line 250), add:

```rust
    /// Initialize an account, handling both fresh and pre-funded cases.
    /// See [`init_account`] for details.
    #[inline(always)]
    pub fn init_account(
        &self,
        payer: &impl AsAccountView,
        account: &mut AccountView,
        lamports: u64,
        space: u64,
        owner: &Address,
        signers: &[Signer],
    ) -> ProgramResult {
        init_account(payer.to_account_view(), account, lamports, space, owner, signers)
    }
```

- [ ] **Step 2: Remove `create_account_with_minimum_balance`**

Delete the `create_account_with_minimum_balance` method from `impl Program<System>` (lines ~219-241). No callers exist in the codebase.

- [ ] **Step 3: Fix `close()` to zero discriminator-sized region, not hardcoded 8**

In `lang/src/accounts/account.rs`, the `close` method (line 199) hardcodes `min(data_len, 8)` for zeroing. This should use the actual discriminator size from `T::DISCRIMINATOR`:

```rust
// Before (line 199):
let zero_len = view.data_len().min(8);

// After:
let zero_len = view.data_len().min(<T as quasar_lang::traits::Discriminator>::DISCRIMINATOR.len());
```

This requires `T: Discriminator`. The existing impl block at line 177 is:
```rust
impl<T: Owner + AsAccountView> Account<T> {
    pub fn owner(&self) -> ... { ... }
    pub fn close(&mut self, ...) { ... }
}
```

**Do NOT add `Discriminator` to this impl block** — that would also constrain `owner()`, breaking callers where `T` doesn't implement `Discriminator`.

Instead, **move `close()` to its own impl block** with the tighter bound:
```rust
// Existing impl — keep owner() here, remove close()
impl<T: Owner + AsAccountView> Account<T> {
    pub fn owner(&self) -> &'static Address { &T::OWNER }
}

// New impl — close() needs Discriminator for zeroing
impl<T: Owner + AsAccountView + crate::traits::Discriminator> Account<T> {
    pub fn close(&mut self, destination: &AccountView) -> Result<(), ProgramError> {
        // ... close body with DISCRIMINATOR.len() zeroing
    }
}
```

Any closeable account already has a discriminator, so this is non-breaking for `close()` callers.

- [ ] **Step 4: Add compile-time assertions (account.rs)**

**Security hardening:** Add `const _` assertions to catch layout drift at compile time.

In `lang/src/accounts/account.rs`, near the top (after imports), add:

```rust
// Compile-time: verify the padding field offset in RuntimeAccount hasn't drifted.
// resize() casts `&mut (*raw).padding` to `*mut i32` — this must be at 0x50.
// If the struct layout changes, this assertion catches it at compile time.
//
// NOTE: `RuntimeAccount` is from the `solana-account-view` crate. If `padding`
// is not public, place this assertion inside `solana-account-view` instead, or
// use a runtime test. The offset is documented at account.rs line 19: 0x50 = 80.
const _: () = {
    assert!(
        core::mem::offset_of!(solana_account_view::RuntimeAccount, padding) == 0x50,
        "RuntimeAccount::padding offset changed — resize() pointer arithmetic is invalid"
    );
};
```

Note: If `offset_of!` is not available (requires Rust 1.77+), use `memoffset` or a const fn that computes the offset. If `RuntimeAccount` doesn't expose `padding` publicly, add this assertion in a test or in the `solana-account-view` crate where the struct is defined.

Also add an Address size/alignment assertion:

```rust
// keys_eq and all 32-byte comparisons assume Address is [u8; 32] with alignment 1.
const _: () = {
    assert!(core::mem::size_of::<solana_address::Address>() == 32);
    assert!(core::mem::align_of::<solana_address::Address>() == 1);
};
```

- [ ] **Step 5: Verify build**

Run: `cargo check`
Expected: clean build

- [ ] **Step 6: Commit**

```
refactor(lang): add init_account to Program<System>, remove dead method, fix close() zeroing

- close() zeros Discriminator::DISCRIMINATOR.len() instead of hardcoded 8
- Compile-time assertions for RuntimeAccount padding offset and Address layout
- Remove dead create_account_with_minimum_balance
```

---

### Task 6: Security hardening — pda.rs, dynamic.rs, validate_ata

**Files:**
- Modify: `lang/src/pda.rs`
- Modify: `derive/src/account/dynamic.rs`
- Modify: `spl/src/validate.rs`

Three independent security fixes that don't touch the derive/accounts/ codegen.

- [ ] **Step 1: Add runtime bounds check on seeds.len() in pda.rs**

**Security fix:** `lang/src/pda.rs` creates `MaybeUninit::<[&[u8]; 19]>::uninit()` arrays and writes `seeds.len()` entries plus extras. The comments claim bounded `n` but there's no runtime check. If seeds exceeds the limit, writes go out of bounds.

The two functions have **different limits** because `based_try_find_program_address` adds a bump seed internally:

- `verify_program_address`: seeds already include bump → max `n = 17` (array uses `n + 2 = 19` slots)
- `based_try_find_program_address`: bump added internally → max `n = 16` (array uses `n + 3 = 19` slots)

At the top of `verify_program_address` (before the `#[cfg]` block):

```rust
pub fn verify_program_address(
    seeds: &[&[u8]],
    program_id: &Address,
    expected: &Address,
) -> Result<(), ProgramError> {
    // seeds includes bump. Array has 19 slots: seeds(max 17) + program_id + PDA_MARKER.
    if seeds.len() > 17 {
        return Err(ProgramError::InvalidSeeds);
    }
    // ... existing code
```

At the top of `based_try_find_program_address` (before the `#[cfg]` block):

```rust
pub fn based_try_find_program_address(
    seeds: &[&[u8]],
    program_id: &Address,
) -> Result<(Address, u8), ProgramError> {
    // bump added internally. Array has 19 slots: seeds(max 16) + bump + program_id + PDA_MARKER.
    if seeds.len() > 16 {
        return Err(ProgramError::InvalidSeeds);
    }
    // ... existing code
```

Use `ProgramError::InvalidSeeds` (exists in the codebase) — not `MaxSeedLengthExceeded` (doesn't exist).

- [ ] **Step 2: Add `checked_mul` in dynamic.rs Vec byte-length calculations**

**Security fix:** `derive/src/account/dynamic.rs` has three sites where `count * core::mem::size_of::<#elem>()` is computed with attacker-controlled `count` from account data:

1. **Line 118** (write path): `#fname.len() * core::mem::size_of::<#elem>()` — this is safe (len comes from program-side Vec, not account data)
2. **Line 262** (validation path): `__count * core::mem::size_of::<#elem>()` — `__count` comes from account data prefix
3. **Line 298** (offset caching path): `__count * core::mem::size_of::<#elem>()` — same source

Replace lines 262 and 298:

```rust
// Line 262 — validation walk:
// Before:
let __byte_len = __count * core::mem::size_of::<#elem>();
// After:
let __byte_len = __count
    .checked_mul(core::mem::size_of::<#elem>())
    .ok_or(ProgramError::InvalidAccountData)?;
```

```rust
// Line 298 — offset caching:
// Before:
__offset += #pb + __count * core::mem::size_of::<#elem>();
// After:
let __byte_len = __count
    .checked_mul(core::mem::size_of::<#elem>())
    .ok_or(ProgramError::InvalidAccountData)?;
__offset += #pb + __byte_len;
```

- [ ] **Step 3: Harden `validate_ata` to verify token_program is a known token program**

**Security fix:** `validate_ata` in `spl/src/validate.rs` derives the ATA address using the `token_program` address, then delegates to `validate_token_account` which checks `view.owner() == token_program`. If the caller passes a garbage `token_program` address, the ATA derivation produces a garbage expected address that won't match — so this is mostly self-protecting.

**However**, for the `Interface<TokenInterface>` path, the token_program address comes from a runtime account. A malicious actor could theoretically craft an account whose address, when used for ATA derivation, produces a collision with the actual account address. While astronomically unlikely (PDA preimage resistance), the defense is trivial: verify the token_program is one of the two known programs.

Add a check at the top of `validate_ata`:

```rust
pub fn validate_ata(
    view: &AccountView,
    wallet: &Address,
    mint: &Address,
    token_program: &Address,
) -> Result<(), ProgramError> {
    // Verify the token program is a known SPL token program.
    // For typed paths (Program<Token>, Program<Token2022>), the address is
    // compile-time verified. For Interface<TokenInterface>, it comes from
    // a runtime account — this check prevents derivation with garbage addresses.
    if unlikely(
        !quasar_lang::keys_eq(token_program, &crate::SPL_TOKEN_ID)
            && !quasar_lang::keys_eq(token_program, &crate::TOKEN_2022_ID)
    ) {
        #[cfg(feature = "debug")]
        quasar_lang::prelude::log("validate_ata: token_program is not SPL Token or Token-2022");
        return Err(ProgramError::IncorrectProgramId);
    }

    let (expected, _) = crate::associated_token::get_associated_token_address_with_program(
        wallet,
        mint,
        token_program,
    );
    // ... rest unchanged
```

- [ ] **Step 4: Verify build**

Run: `cargo check`
Expected: clean build

- [ ] **Step 5: Commit**

```
fix: security hardening — pda seeds bounds, dynamic checked_mul, validate_ata

- pda.rs: runtime bounds check on seeds.len() (max 17)
- dynamic.rs: checked_mul for Vec byte-length from account data
- validate_ata: verify token_program is SPL_TOKEN or TOKEN_2022
```

---

### Task 7: Final verification

- [ ] **Step 1: Full workspace build**

Run: `cargo check`
Expected: all crates build clean

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path lang/Cargo.toml`
Expected: all tests pass

- [ ] **Step 3: Build test programs**

Run: `cargo check --manifest-path tests/programs/test-misc/Cargo.toml`
Expected: compiles clean — this exercises the derive macro codegen paths including optional accounts

- [ ] **Step 4: Review diff**

Run: `git diff --stat HEAD~6` (or however many commits)
Expected: net reduction in `fields.rs` lines, new focused `field_kind.rs` and `init.rs`, clean separation, security hardening across pda.rs/dynamic.rs/validate.rs
