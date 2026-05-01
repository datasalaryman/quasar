//! Phased UFCS codegen — the core of v3.
//!
//! Generated parse body shape:
//!
//! ```text
//! // Phase 1: pre-load
//! let __ctx = OpCtx::new(...);
//! // AddressVerify, init::Op::before_load, before_load groups
//!
//! // Phase 2: load all
//! let field_a = <Ty>::load(field_a, "field_a")?;
//! let mut field_b = <Ty>::load_mut(field_b, "field_b")?;
//!
//! // Phase 3a: after_load (shared ref on locals)
//! <token::Op<'_> as AccountOp<Ty>>::after_load(&op, &field_b, &__ctx)?;
//!
//! // Phase 3b: after_load_mut (mut ref on owned locals)
//! <realloc::Op<'_> as AccountOp<Ty>>::after_load_mut(&op, &mut field_b, &__ctx)?;
//!
//! // Phase 3c: user checks
//! check_address_match(...)?;
//!
//! Ok((Self { field_a, field_b }, bumps))
//! ```

use {
    super::{
        super::resolve::{FieldKind, FieldSemantics, UserCheck},
        ops::{
            exit_arg, typed_arg,
            OpEmitCtx,
        },
    },
    crate::helpers::strip_generics,
    quote::{format_ident, quote},
};

pub(crate) fn emit_parse_body(
    semantics: &[FieldSemantics],
    cx: &super::EmitCx,
) -> syn::Result<proc_macro2::TokenStream> {
    let op_ctx = OpEmitCtx {
        field_names: semantics.iter().map(|s| s.core.ident.to_string()).collect(),
    };
    // Only emit OpCtx when ops/init/lifecycle/realloc actually need it.
    let needs_ctx = semantics.iter().any(|sem| {
        !sem.groups.is_empty()
            || sem.init.is_some()
            || sem.realloc.is_some()
            || (sem.core.is_mut && sem.core.kind == FieldKind::Single && has_field_lifecycle(sem))
    });
    let rent_field = find_rent_sysvar_field(semantics);
    let ctx_init = if !needs_ctx {
        quote! {}
    } else if let Some(rent_ident) = &rent_field {
        quote! {
            let __ctx = quasar_lang::ops::OpCtx::new_with_rent(
                unsafe { &*(__program_id as *const quasar_lang::prelude::Address) },
                unsafe {
                    core::clone::Clone::clone(
                        <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::from_bytes_unchecked(
                            #rent_ident.borrow_unchecked()
                        )
                    )
                },
            );
        }
    } else {
        // No Sysvar<Rent> field. If init/realloc exist, rent will be needed —
        // fetch from sysvar. Otherwise, rent stays None.
        let needs_rent = semantics.iter().any(|sem| {
            sem.init.is_some()
                || sem.realloc.is_some()
                || (sem.core.is_mut
                    && sem.core.kind == FieldKind::Single
                    && has_field_lifecycle(sem))
        });
        if needs_rent {
            quote! {
                let __ctx = quasar_lang::ops::OpCtx::new_fetch_rent(unsafe {
                    &*(__program_id as *const quasar_lang::prelude::Address)
                })?;
            }
        } else {
            quote! {
                let __ctx = quasar_lang::ops::OpCtx::new(unsafe {
                    &*(__program_id as *const quasar_lang::prelude::Address)
                });
            }
        }
    };
    let bump_vars = emit_bump_vars(semantics);

    // Phase ordering:
    // 1. Load non-init fields (they exist on-chain, data available)
    // 2. Address verify + init CPI for init fields (can reference loaded non-init
    //    fields like config.namespace)
    // 3. Load init fields (now created by init CPI)
    // 4. Ops, validation, user checks
    let load_non_init = emit_phase2_filtered(semantics, false);
    let init_phase = emit_init_phase(semantics, &op_ctx)?;
    let load_init = emit_phase2_filtered(semantics, true);
    let phase3 = emit_phase3(semantics, &op_ctx);
    let bump_init = emit_bump_init(semantics, &cx.bumps_name);

    let construct_fields: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .map(|sem| {
            let ident = &sem.core.ident;
            quote! { #ident }
        })
        .collect();

    Ok(quote! {
        #bump_vars
        #ctx_init
        #(#load_non_init)*
        #(#init_phase)*
        #(#load_init)*
        #(#phase3)*
        Ok((Self { #(#construct_fields,)* }, #bump_init))
    })
}


// ==== Init phase: address verify + init CPI (runs after non-init fields
// loaded) ====

fn emit_init_phase(
    semantics: &[FieldSemantics],
    op_ctx: &OpEmitCtx,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut stmts = Vec::new();

    for sem in semantics {
        let ident = &sem.core.ident;

        // Address verification for init fields.
        // Runs after non-init fields are loaded, so expressions like
        // config.namespace work via Deref on loaded Account<T> fields.
        if sem.has_init() {
            if let Some(address_expr) = &sem.address {
                let bump_var = format_ident!("__bumps_{}", ident);
                let addr_var = format_ident!("__addr_{}", ident);
                stmts.push(quote! {
                    let #addr_var = #address_expr;
                    #bump_var = quasar_lang::address::AddressVerify::verify(
                        &#addr_var, #ident.address(), __program_id,
                    )?;
                });
            }
        }

        // init → init::Op::before_load
        if let Some(init) = &sem.init {
            stmts.push(emit_init_before_load(sem, init, op_ctx)?);
        }
    }

    Ok(stmts)
}

fn emit_init_before_load(
    sem: &FieldSemantics,
    init: &super::super::resolve::InitDirective,
    op_ctx: &OpEmitCtx,
) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &sem.core.ident;
    let ty = &sem.core.effective_ty;

    let payer = sem
        .payer
        .as_ref()
        .ok_or_else(|| syn::Error::new_spanned(&sem.core.field, "init requires `payer`"))?;

    // Space from Space::SPACE for program-owned accounts.
    // When init contributors exist (token, mint, ata_init), space is 0
    // because the SPL init CPI handles allocation.
    let has_param_contributors = !sem.init_contributors.is_empty();
    let space = if has_param_contributors {
        quote! { 0u64 }
    } else {
        quote! {
            <
                #ty
                as quasar_lang::traits::Space
            >::SPACE as u64
        }
    };
    let idempotent = init.idempotent;

    // Init params: when contributors exist, construct mutable default then
    // apply each contributor. Otherwise use immutable default.
    let has_contributors = !sem.init_contributors.is_empty();
    let params_block = if has_contributors {
        quote! {
            let mut __init_params = <
                #ty
                as quasar_lang::account_init::AccountInit
            >::InitParams::default();
        }
    } else {
        // No groups → default params (typically () for plain accounts)
        quote! {
            let __init_params = <
                #ty
                as quasar_lang::account_init::AccountInit
            >::InitParams::default();
        }
    };

    // Direct capability trait calls for init contributors.
    let spl_crate = format_ident!("quasar_{}", "spl");
    let contributor_calls: Vec<proc_macro2::TokenStream> = sem
        .init_contributors
        .iter()
        .map(|group| {
            emit_init_contributor_call(ty, group, op_ctx, &spl_crate)
        })
        .collect();

    let init_call = quote! {
        let __init_op = quasar_lang::ops::init::Op {
            payer: #payer.to_account_view(),
            space: #space,
            signers: __signers,
            params: __init_params,
            idempotent: #idempotent,
        };
        __init_op.apply::<#ty>(#ident, &__ctx)?;
    };

    let inner_body = quote! {
            #params_block
            #(#contributor_calls)*
            #init_call
    };

    // Build signers from address spec if present (PDA init).
    // AddressVerify ran before this, so __bumps_<ident> is populated.
    let body = if sem.address.is_some() {
        let bump_var = format_ident!("__bumps_{}", ident);
        let addr_var = format_ident!("__addr_{}", ident);
        quote! {
            let __bump_ref: &[u8] = &[#bump_var];
            quasar_lang::address::AddressVerify::with_signer_seeds(
                &#addr_var,
                __bump_ref,
                |__maybe_signer| -> Result<(), quasar_lang::prelude::ProgramError> {
                    let __signers = match &__maybe_signer {
                        Some(__signer) => core::slice::from_ref(__signer),
                        None => &[] as &[quasar_lang::cpi::Signer<'_, '_>],
                    };
                    #inner_body
                    Ok(())
                },
            )?;
        }
    } else {
        quote! {
            let __signers: &[quasar_lang::cpi::Signer<'_, '_>] = &[];
            #inner_body
        }
    };

    // For idempotent init, gate the entire block on is_system_program.
    // When the account already exists, skip all init param construction,
    // op struct building, and before_load — pure zero overhead on the hot path.
    // Non-idempotent init must always run (needs the error on existing accounts).
    if idempotent {
        Ok(quote! {
            if quasar_lang::is_system_program(#ident.owner()) {
                #body
            }
        })
    } else {
        Ok(quote! { { #body } })
    }
}

// ==== Phase 2: load (split into non-init first, then init) ====

fn emit_phase2_filtered(
    semantics: &[FieldSemantics],
    init_only: bool,
) -> Vec<proc_macro2::TokenStream> {
    semantics
        .iter()
        .filter(|sem| sem.core.kind == FieldKind::Single)
        .filter(|sem| sem.has_init() == init_only)
        .map(emit_one_load)
        .collect()
}

fn emit_one_load(sem: &FieldSemantics) -> proc_macro2::TokenStream {
    let ident = &sem.core.ident;
    let ty = &sem.core.effective_ty;
    let field_name_str = ident.to_string();

    if sem.core.dynamic {
        let inner_ty = sem.core.inner_ty.as_ref().unwrap_or(ty);
        let base = strip_generics(inner_ty);
        return quote! { let #ident = #base::from_account_view(#ident)?; };
    }

    // Always use validated load — no unchecked bypass.
    // init::Op writes discriminator + data, so load() succeeds after valid init.

    if sem.core.optional {
        return if sem.core.is_mut {
            quote! {
                let mut #ident = if quasar_lang::keys_eq(#ident.address(), __program_id) {
                    None
                } else {
                    Some(<#ty as quasar_lang::account_load::AccountLoad>::load_mut(#ident, #field_name_str)?)
                };
            }
        } else {
            quote! {
                let #ident = if quasar_lang::keys_eq(#ident.address(), __program_id) {
                    None
                } else {
                    Some(<#ty as quasar_lang::account_load::AccountLoad>::load(#ident, #field_name_str)?)
                };
            }
        };
    }

    if sem.core.is_mut {
        quote! {
            let mut #ident = <#ty as quasar_lang::account_load::AccountLoad>::load_mut(#ident, #field_name_str)?;
        }
    } else {
        quote! {
            let #ident = <#ty as quasar_lang::account_load::AccountLoad>::load(#ident, #field_name_str)?;
        }
    }
}

// ==== Phase 3: validate + mutate + user checks ====

fn emit_phase3(semantics: &[FieldSemantics], op_ctx: &OpEmitCtx) -> Vec<proc_macro2::TokenStream> {
    let mut stmts = Vec::new();

    for sem in semantics {
        let ident = &sem.core.ident;
        let ty = &sem.core.effective_ty;
        let is_optional = sem.core.optional;

        // Phase 3a: constraint checks — direct capability trait calls.
        for group in &sem.constraints {
            let call = emit_constraint_call(ty, ident, group, op_ctx);
            stmts.push(wrap_optional(is_optional, ident, &call, false));
        }

        // Phase 3b: after_load_mut (realloc + lifecycle).
        if sem.core.is_mut && sem.core.kind == FieldKind::Single {
            // Realloc op (Phase 3b) — emitted when field has `realloc = expr`
            if let Some(realloc_expr) = &sem.realloc {
                let payer = sem.payer.as_ref().expect("realloc requires payer");
                let call = quote! {
                    {
                        let __realloc_op = quasar_lang::ops::realloc::Op {
                            space: (#realloc_expr) as usize,
                            payer: #payer.to_account_view(),
                        };
                        __realloc_op.apply::<#ty>(&mut #ident, &__ctx)?;
                    }
                };
                stmts.push(wrap_optional(is_optional, ident, &call, true));
            }

            if has_field_lifecycle(sem) {
                // AccountLoad before_init hook. Migration uses this to
                // grow the account before handler code writes the target
                // layout.
                let payer_option = match &sem.payer {
                    Some(p) => quote! { Some(#p.to_account_view()) },
                    None => quote! { None },
                };
                let lifecycle_call = quote! {
                    if <#ty as quasar_lang::account_load::AccountLoad>::HAS_BEFORE_INIT {
                        quasar_lang::account_load::AccountLoad::before_init(
                            &mut #ident,
                            #payer_option,
                            &__ctx,
                        )?;
                    }
                };
                stmts.push(wrap_optional(is_optional, ident, &lifecycle_call, true));
            }
        }

        // Phase 3c: address verification for NON-init fields (after load).
        // Account<T>, InterfaceAccount<T>, Migration<From,To> use verify_existing
        // (fast path) — safe because owner+disc validation during load guarantees
        // the program created this account with the canonical bump.
        // UncheckedAccount, SystemAccount, Signer use full verify() — no owner
        // validation means non-canonical bumps could exist.
        if !sem.has_init() {
            if let Some(address_expr) = &sem.address {
                let bump_var = format_ident!("__bumps_{}", ident);
                let use_fast_path = is_validated_account_type(&sem.core.effective_ty);
                let verify_method = if use_fast_path {
                    quote! { verify_existing }
                } else {
                    quote! { verify }
                };
                let call = quote! {
                    {
                        let __addr = #address_expr;
                        #bump_var = quasar_lang::address::AddressVerify::#verify_method(
                            &__addr, #ident.to_account_view().address(), __program_id,
                        )?;
                    }
                };
                stmts.push(wrap_optional(is_optional, ident, &call, false));
            }
        }

        // Phase 3d: user checks
        for check in &sem.user_checks {
            let check_stmts = emit_user_check(sem, check);
            let combined = quote! { #(#check_stmts)* };
            stmts.push(wrap_optional(is_optional, ident, &combined, false));
        }

    }

    stmts
}

/// Fields relevant to each check context struct.
const TOKEN_CHECK_FIELDS: &[&str] = &["mint", "authority", "token_program"];
const MINT_CHECK_FIELDS: &[&str] = &["decimals", "authority", "freeze_authority", "token_program"];
const ATA_CHECK_FIELDS: &[&str] = &["mint", "authority", "token_program"];

/// Emit a direct capability trait call for a constraint group.
fn emit_constraint_call(
    ty: &syn::Type,
    ident: &syn::Ident,
    group: &super::super::resolve::GroupDirective,
    op_ctx: &OpEmitCtx,
) -> proc_macro2::TokenStream {
    let name = group
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();

    let spl_crate = format_ident!("quasar_{}", "spl");

    // Filter group args to only those relevant for the check context struct.
    let check_fields: &[&str] = match name.as_str() {
        "token" => TOKEN_CHECK_FIELDS,
        "mint" => MINT_CHECK_FIELDS,
        "associated_token" | "ata_init" => ATA_CHECK_FIELDS,
        _ => &[],
    };

    let args: Vec<proc_macro2::TokenStream> = group
        .args
        .iter()
        .filter(|a| a.key != "target" && check_fields.contains(&a.key.to_string().as_str()))
        .map(|arg| {
            let key = &arg.key;
            let value = typed_arg(arg, op_ctx);
            quote! { #key: #value }
        })
        .collect();

    match name.as_str() {
        "token" => quote! {
            <#ty as #spl_crate::ops::capabilities::TokenCheck>::check_token_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::TokenCheckCtx { #(#args,)* },
            )?;
        },
        "mint" => quote! {
            <#ty as #spl_crate::ops::capabilities::MintCheck>::check_mint_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::MintCheckCtx { #(#args,)* },
            )?;
        },
        "associated_token" | "ata_init" => quote! {
            <#ty as #spl_crate::ops::capabilities::AtaCheck>::check_ata_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::AtaCheckCtx { #(#args,)* },
            )?;
        },
        _ => unreachable!("classify_group ensures only known groups reach here"),
    }
}

/// Emit a direct capability trait call for an init contributor group.
fn emit_init_contributor_call(
    ty: &syn::Type,
    group: &super::super::resolve::GroupDirective,
    op_ctx: &OpEmitCtx,
    spl_crate: &syn::Ident,
) -> proc_macro2::TokenStream {
    let name = group
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();

    // Build context struct fields from group args (excluding target).
    let args: Vec<proc_macro2::TokenStream> = group
        .args
        .iter()
        .filter(|a| a.key != "target")
        .map(|arg| {
            let key = &arg.key;
            let value = typed_arg(arg, op_ctx);
            quote! { #key: #value }
        })
        .collect();

    match name.as_str() {
        "token" => quote! {
            <#ty as #spl_crate::ops::capabilities::TokenInitContributor>::apply_token_init(
                &mut __init_params,
                #spl_crate::ops::ctx::TokenInitCtx { #(#args,)* },
            )?;
        },
        "mint" => quote! {
            <#ty as #spl_crate::ops::capabilities::MintInitContributor>::apply_mint_init(
                &mut __init_params,
                #spl_crate::ops::ctx::MintInitCtx { #(#args,)* },
            )?;
        },
        "ata_init" => quote! {
            <#ty as #spl_crate::ops::capabilities::AtaInitContributor>::apply_ata_init(
                &mut __init_params,
                #spl_crate::ops::ctx::AtaInitCtx { #(#args,)* },
            )?;
        },
        _ => unreachable!("only ConstraintAndInit ops reach init_contributors"),
    }
}

/// Emit a direct capability trait call for an exit action group.
fn emit_exit_action_call(
    ty: &syn::Type,
    field: &syn::Ident,
    group: &super::super::resolve::GroupDirective,
    op_ctx: &OpEmitCtx,
    spl_crate: &syn::Ident,
) -> proc_macro2::TokenStream {
    let name = group
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();

    // Helper: look up an arg value by key name.
    let arg_val = |key: &str| -> proc_macro2::TokenStream {
        group
            .args
            .iter()
            .find(|a| a.key == key)
            .map(|a| exit_arg(a, op_ctx))
            .unwrap_or_else(|| quote! { compile_error!(concat!("missing arg: ", #key)) })
    };

    match name.as_str() {
        "sweep" => {
            let receiver = arg_val("receiver");
            let mint = arg_val("mint");
            let authority = arg_val("authority");
            let token_program = arg_val("token_program");
            let trait_name = format_ident!("Token{}", "Sweep");
            quote! {
                <#ty as #spl_crate::ops::sweep::#trait_name>::sweep(
                    self.#field.to_account_view(),
                    #receiver,
                    #mint,
                    #authority,
                    #token_program,
                )?;
            }
        }
        "close" => {
            let dest = arg_val("dest");
            let authority = arg_val("authority");
            let token_program = arg_val("token_program");
            let trait_name = format_ident!("Token{}", "Close");
            quote! {
                {
                    let __view = unsafe {
                        <#ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                            &mut self.#field
                        )
                    };
                    <#ty as #spl_crate::ops::close::#trait_name>::close(
                        __view,
                        #dest,
                        #authority,
                        #token_program,
                    )?;
                }
            }
        }
        "close_program" => {
            let dest = arg_val("dest");
            let trait_name = format_ident!("{}Close", "Account");
            quote! {
                {
                    let __view = unsafe {
                        <#ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                            &mut self.#field
                        )
                    };
                    <#ty as quasar_lang::ops::close_program::#trait_name>::close(
                        __view,
                        #dest,
                    )?;
                }
            }
        }
        _ => unreachable!("only Exit ops reach exit_actions"),
    }
}

/// Wrap a code block in `if let Some(ref field) = field { ... }` for optional
/// accounts. `needs_mut` uses `ref mut` instead.
fn wrap_optional(
    is_optional: bool,
    ident: &syn::Ident,
    body: &proc_macro2::TokenStream,
    needs_mut: bool,
) -> proc_macro2::TokenStream {
    if !is_optional {
        return body.clone();
    }
    if needs_mut {
        quote! {
            if let Some(ref mut #ident) = #ident {
                #body
            }
        }
    } else {
        quote! {
            if let Some(ref #ident) = #ident {
                #body
            }
        }
    }
}

fn emit_user_check(sem: &FieldSemantics, check: &UserCheck) -> Vec<proc_macro2::TokenStream> {
    let field_ident = &sem.core.ident;
    let mut stmts = Vec::new();

    match check {
        UserCheck::HasOne { targets, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::HasOneMismatch.into() },
            };
            for target in targets {
                stmts.push(quote! {
                    quasar_lang::validation::check_address_match(
                        &#field_ident.#target,
                        #target.to_account_view().address(),
                        #err,
                    )?;
                });
            }
        }
        UserCheck::Address { expr, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::AddressMismatch.into() },
            };
            stmts.push(quote! {
                quasar_lang::validation::check_address_match(
                    #field_ident.to_account_view().address(),
                    &#expr,
                    #err,
                )?;
            });
        }
        UserCheck::Constraints { exprs, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::ConstraintViolation.into() },
            };
            for expr in exprs {
                stmts.push(quote! {
                    quasar_lang::validation::check_constraint(#expr, #err)?;
                });
            }
        }
    }

    stmts
}

// ==== Epilogue (Phase 4) ====

pub(crate) fn emit_epilogue(
    semantics: &[FieldSemantics],
    op_ctx: &OpEmitCtx,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut exit_stmts = Vec::new();
    let spl_crate = format_ident!("quasar_{}", "spl");

    for sem in semantics {
        let ty = &sem.core.effective_ty;
        let field = &sem.core.ident;

        // Exit actions — direct capability trait calls, sorted (sweep before close).
        for group in &sem.exit_actions {
            exit_stmts.push(emit_exit_action_call(ty, field, group, op_ctx, &spl_crate));
        }

        // AccountLoad exit_validation hook for types with lifecycle behavior.
        if sem.core.is_mut && sem.core.kind == FieldKind::Single && has_field_lifecycle(sem) {
            let payer_option = match &sem.payer {
                Some(p) => quote! { Some(self.#p.to_account_view()) },
                None => quote! { None },
            };
            exit_stmts.push(quote! {
                if <#ty as quasar_lang::account_load::AccountLoad>::HAS_EXIT_VALIDATION {
                    quasar_lang::account_load::AccountLoad::exit_validation(
                        &mut self.#field,
                        #payer_option,
                        &__ctx,
                    )?;
                }
            });
        }
    }

    if exit_stmts.is_empty() {
        return Ok(quote! {});
    }

    Ok(quote! {
        #[inline(always)]
        fn epilogue(&mut self) -> Result<(), ProgramError> {
            let __ctx = quasar_lang::ops::OpCtx::new(&crate::ID);
            #(#exit_stmts)*
            Ok(())
        }
    })
}

pub(crate) fn emit_has_epilogue(semantics: &[FieldSemantics]) -> proc_macro2::TokenStream {
    let mut exprs: Vec<proc_macro2::TokenStream> = Vec::new();

    for sem in semantics {
        // Exit actions are known statically — if any exist, epilogue is needed.
        if !sem.exit_actions.is_empty() {
            exprs.push(quote! { true });
        }

        // AccountLoad exit_validation for types with lifecycle behavior.
        if sem.core.is_mut && sem.core.kind == FieldKind::Single && has_field_lifecycle(sem) {
            let ty = &sem.core.effective_ty;
            exprs.push(quote! {
                <#ty as quasar_lang::account_load::AccountLoad>::HAS_EXIT_VALIDATION
            });
        }
    }

    if exprs.is_empty() {
        quote! { false }
    } else {
        quote! { #(#exprs)||* }
    }
}

fn has_field_lifecycle(sem: &FieldSemantics) -> bool {
    match &sem.core.effective_ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Migration"),
        _ => false,
    }
}

// ==== Helpers ====

fn emit_bump_vars(semantics: &[FieldSemantics]) -> proc_macro2::TokenStream {
    let vars: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .map(|sem| {
            let var = format_ident!("__bumps_{}", sem.core.ident);
            quote! { let mut #var: u8 = 0; }
        })
        .collect();

    quote! { #(#vars)* }
}

fn emit_bump_init(
    semantics: &[FieldSemantics],
    bumps_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    let inits: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .flat_map(|sem| {
            let name = &sem.core.ident;
            let var = format_ident!("__bumps_{}", name);
            let arr_name = format_ident!("__{}_bump", name);
            vec![quote! { #name: #var }, quote! { #arr_name: [#var] }]
        })
        .collect();

    if inits.is_empty() {
        quote! { #bumps_name }
    } else {
        quote! { #bumps_name { #(#inits,)* } }
    }
}

pub(crate) fn emit_bump_struct_def(
    semantics: &[FieldSemantics],
    cx: &super::EmitCx,
) -> proc_macro2::TokenStream {
    let bumps_name = &cx.bumps_name;
    let fields: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .flat_map(|sem| {
            let name = &sem.core.ident;
            let arr_name = format_ident!("__{}_bump", name);
            vec![quote! { pub #name: u8 }, quote! { pub #arr_name: [u8; 1] }]
        })
        .collect();

    if fields.is_empty() {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name; }
    } else {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name { #(#fields,)* } }
    }
}

/// Find a field with type `Sysvar<Rent>` in the accounts struct.
fn find_rent_sysvar_field(semantics: &[FieldSemantics]) -> Option<syn::Ident> {
    for sem in semantics {
        if let syn::Type::Path(tp) = &sem.core.effective_ty {
            if let Some(last) = tp.path.segments.last() {
                if last.ident == "Sysvar" {
                    if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(syn::Type::Path(inner)) = arg {
                                if inner
                                    .path
                                    .segments
                                    .last()
                                    .is_some_and(|s| s.ident == "Rent")
                                {
                                    return Some(sem.core.ident.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Returns true for account types with owner + discriminator validation
/// (Account<T>, InterfaceAccount<T>, Migration<From,To>). These are safe for
/// verify_existing because the program created them with the canonical bump.
fn is_validated_account_type(ty: &syn::Type) -> bool {
    use crate::helpers::extract_generic_inner_type;
    extract_generic_inner_type(ty, "Account").is_some()
        || extract_generic_inner_type(ty, "InterfaceAccount").is_some()
        || extract_generic_inner_type(ty, "Migration").is_some()
}
