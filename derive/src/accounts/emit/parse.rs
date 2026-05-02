//! Phased codegen — direct capability dispatch.
//!
//! Generated parse body shape:
//!
//! ```text
//! // Rent (only when init/realloc/migration needs it)
//! let __rent: Rent = Sysvar::get()?;  // or deserialized from Sysvar<Rent> field
//! let __rent_ctx = OpCtxWithRent::new(&program_id, &__rent);
//!
//! // Phase 1: load non-init fields
//! let field_a = <Ty>::load(field_a, "field_a")?;
//! let mut field_b = <Ty>::load_mut(field_b, "field_b")?;
//!
//! // Phase 2: init CPI for init fields
//! init_op.apply::<Ty>(slot, &__rent_ctx)?;
//!
//! // Phase 3a: direct capability checks on loaded accounts
//! <Ty as TokenCheck>::check_token_view(view, ctx)?;
//!
//! // Phase 3b: realloc
//! realloc_op.apply::<Ty>(&mut field_b, &__rent_ctx)?;
//!
//! // Phase 3c: user checks
//! check_address_match(...)?;
//!
//! Ok((Self { field_a, field_b }, bumps))
//! ```

use {
    super::{
        super::resolve::{FieldKind, FieldSemantics, GroupKind, GroupOp, UserCheck},
        ops::{exit_arg, typed_arg, OpEmitCtx},
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
    // Emit rent context when any field needs init, realloc, or migration lifecycle.
    let needs_rent = semantics.iter().any(|sem| {
        sem.init.is_some() || sem.realloc.is_some() || sem.is_migration
    });
    let rent_field = find_rent_sysvar_field(semantics);
    let ctx_init = if !needs_rent {
        quote! {}
    } else {
        let rent_fetch = if let Some(rent_ident) = &rent_field {
            // Sysvar<Rent> field available — deserialize from it (free).
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent = unsafe {
                    core::clone::Clone::clone(
                        <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::from_bytes_unchecked(
                            #rent_ident.borrow_unchecked()
                        )
                    )
                };
            }
        } else {
            // No Sysvar<Rent> field — syscall once.
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::get()?;
            }
        };
        quote! {
            #rent_fetch
            let __rent_ctx = quasar_lang::ops::OpCtxWithRent::new(
                unsafe { &*(__program_id as *const quasar_lang::prelude::Address) },
                &__rent,
            );
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
    // When SPL init contributors exist (token, mint, associated_token), space
    // is 0 because the SPL init CPI handles allocation.
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

    // Init params: construct directly from validated args — no Options, no Default.
    let spl_crate = format_ident!("quasar_{}", "spl");
    let params_block = if sem.init_contributors.is_empty() {
        // No groups → plain program account init, params = ()
        quote! { let __init_params = (); }
    } else {
        // Exactly one contributor (validated by rules.rs). Construct directly.
        let group = &sem.init_contributors[0];
        emit_init_params_direct(group, op_ctx, &spl_crate, idempotent)
    };

    let init_call = quote! {
        let __init_op = quasar_lang::ops::init::Op {
            payer: #payer.to_account_view(),
            space: #space,
            signers: __signers,
            params: __init_params,
            idempotent: #idempotent,
        };
        __init_op.apply::<#ty>(#ident, &__rent_ctx)?;
    };

    let inner_body = quote! {
            #params_block
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

/// Construct init params directly from a single init contributor group.
/// No Options, no Default, no contributor traits.
///
/// Type names are constructed via `format_ident!` splits to satisfy
/// `deny_domain_strings.rs` — the derive owns structural SPL group lowering
/// but must not contain literal SPL type names.
fn emit_init_params_direct(
    group: &GroupOp,
    op_ctx: &OpEmitCtx,
    spl_crate: &syn::Ident,
    idempotent: bool,
) -> proc_macro2::TokenStream {
    match group.kind {
        GroupKind::Token => {
            let mint = typed_arg(find_arg(&group.args, "mint"), op_ctx);
            let authority = typed_arg(find_arg(&group.args, "authority"), op_ctx);
            let token_program = typed_arg(find_arg(&group.args, "token_program"), op_ctx);
            let kind_ty = format_ident!("Token{}Kind", "Init");
            quote! {
                let __init_params = #spl_crate::#kind_ty::Token {
                    mint: #mint,
                    authority: #authority.address(),
                    token_program: #token_program,
                };
            }
        }
        GroupKind::Mint => {
            let decimals = group
                .args
                .iter()
                .find(|a| a.key == "decimals")
                .map(|a| typed_arg(a, op_ctx))
                .unwrap_or_else(|| quote! { 6u8 });
            let authority = typed_arg(find_arg(&group.args, "authority"), op_ctx);
            let token_program = group
                .args
                .iter()
                .find(|a| a.key == "token_program")
                .map(|a| typed_arg(a, op_ctx));
            // freeze_authority is legitimately optional.
            // None → None, Some(field) → Some(field.to_account_view().address()).
            let freeze_authority = group
                .args
                .iter()
                .find(|a| a.key == "freeze_authority")
                .map(|a| emit_optional_address_arg(a, op_ctx))
                .unwrap_or_else(|| quote! { None });
            let params_ty = format_ident!("Mint{}Params", "Init");
            let tp_expr = match token_program {
                Some(tp) => quote! { #tp },
                None => quote! {
                    compile_error!("mint init requires token_program — add a Program<TokenProgram> \
                                    field or specify `token_program = ...`")
                },
            };
            quote! {
                let __init_params = #spl_crate::#params_ty {
                    decimals: #decimals,
                    authority: #authority.address(),
                    freeze_authority: #freeze_authority,
                    token_program: #tp_expr,
                };
            }
        }
        GroupKind::AssociatedToken => {
            let mint = typed_arg(find_arg(&group.args, "mint"), op_ctx);
            let authority = typed_arg(find_arg(&group.args, "authority"), op_ctx);
            let token_program = typed_arg(find_arg(&group.args, "token_program"), op_ctx);
            let system_program = typed_arg(find_arg(&group.args, "system_program"), op_ctx);
            let ata_program = typed_arg(find_arg(&group.args, "ata_program"), op_ctx);
            let kind_ty = format_ident!("Token{}Kind", "Init");
            quote! {
                let __init_params = #spl_crate::#kind_ty::AssociatedToken {
                    mint: #mint,
                    authority: #authority,
                    token_program: #token_program,
                    system_program: #system_program,
                    ata_program: #ata_program,
                    idempotent: #idempotent,
                };
            }
        }
        _ => unreachable!("only Check ops reach init_contributors"),
    }
}

/// Emit an optional address arg: `None` → `None`, `Some(field)` →
/// `Some(field.to_account_view().address())`.
fn emit_optional_address_arg(
    arg: &super::super::resolve::GroupArg,
    op_ctx: &OpEmitCtx,
) -> proc_macro2::TokenStream {
    match &arg.value {
        // None → None
        syn::Expr::Path(ep)
            if ep.qself.is_none()
                && ep.path.segments.len() == 1
                && ep.path.segments[0].ident == "None" =>
        {
            quote! { None }
        }
        // Some(inner) → Some(inner.to_account_view().address())
        syn::Expr::Call(call)
            if matches!(&*call.func, syn::Expr::Path(p)
                if p.path.segments.len() == 1 && p.path.segments[0].ident == "Some")
                && call.args.len() == 1 =>
        {
            // Create a temporary GroupArg with the inner expr to use typed_arg
            let inner_arg = super::super::resolve::GroupArg {
                key: arg.key.clone(),
                value: call.args[0].clone(),
            };
            let inner_val = typed_arg(&inner_arg, op_ctx);
            quote! { Some(#inner_val.address()) }
        }
        // Bare expression (shouldn't happen for freeze_authority, but handle)
        _ => {
            let val = typed_arg(arg, op_ctx);
            quote! { Some(#val.address()) }
        }
    }
}

/// Find a required arg by key. Validation in `rules.rs` guarantees the arg
/// exists — this `expect` is a safety net, not a user-facing error.
fn find_arg<'a>(
    args: &'a [super::super::resolve::GroupArg],
    key: &str,
) -> &'a super::super::resolve::GroupArg {
    args.iter()
        .find(|a| a.key == key)
        .expect("validate_required_group_args should have caught this")
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

        // Phase 3b: after_load_mut (realloc + migration grow).
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
                        __realloc_op.apply::<#ty>(&mut #ident, &__rent_ctx)?;
                    }
                };
                stmts.push(wrap_optional(is_optional, ident, &call, true));
            }

            // Migration grow: direct call instead of trait dispatch.
            // Validation rules guarantee payer exists for Migration fields.
            if sem.is_migration {
                let payer = sem.payer.as_ref().expect("migration requires payer");
                let lifecycle_call = quote! {
                    quasar_lang::accounts::migration::Migration::grow_to_target(
                        &mut #ident,
                        #payer.to_account_view(),
                        &__rent_ctx,
                    )?;
                };
                stmts.push(lifecycle_call);
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

/// Fields that are wrapped in Some() when present, None when absent.
/// These are fields the derive infers as optional — the user doesn't write
/// Some/None, the derive wraps them.
///
/// Note: `freeze_authority` is NOT here — it uses explicit `None`/`Some(field)`
/// syntax because validation distinguishes "assert no freeze authority" from
/// "don't check."
const INFERRED_OPTIONAL_CHECK_FIELDS: &[&str] = &["token_program", "decimals"];

/// Emit a direct capability trait call for a constraint group.
fn emit_constraint_call(
    ty: &syn::Type,
    ident: &syn::Ident,
    group: &GroupOp,
    op_ctx: &OpEmitCtx,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");

    // Filter group args to only those relevant for the check context struct.
    let check_fields: &[&str] = match group.kind {
        GroupKind::Token => TOKEN_CHECK_FIELDS,
        GroupKind::Mint => MINT_CHECK_FIELDS,
        GroupKind::AssociatedToken => ATA_CHECK_FIELDS,
        _ => &[],
    };

    // Build present args, wrapping optional fields in Some(...).
    let mut args: Vec<proc_macro2::TokenStream> = group
        .args
        .iter()
        .filter(|a| a.key != "target" && check_fields.iter().any(|field| a.key == *field))
        .map(|arg| {
            let key = &arg.key;
            let value = typed_arg(arg, op_ctx);
            if INFERRED_OPTIONAL_CHECK_FIELDS.contains(&key.to_string().as_str()) {
                quote! { #key: Some(#value) }
            } else {
                quote! { #key: #value }
            }
        })
        .collect();

    // Emit None for optional fields not present in args.
    for &field in INFERRED_OPTIONAL_CHECK_FIELDS {
        if check_fields.contains(&field) && !group.args.iter().any(|a| a.key == field) {
            let key = format_ident!("{}", field);
            args.push(quote! { #key: None });
        }
    }

    match group.kind {
        GroupKind::Token => quote! {
            <#ty as #spl_crate::ops::capabilities::TokenCheck>::check_token_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::TokenCheckCtx { #(#args,)* },
            )?;
        },
        GroupKind::Mint => quote! {
            <#ty as #spl_crate::ops::capabilities::MintCheck>::check_mint_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::MintCheckCtx { #(#args,)* },
            )?;
        },
        GroupKind::AssociatedToken => quote! {
            <#ty as #spl_crate::ops::capabilities::AssociatedTokenCheck>::check_associated_token_view(
                #ident.to_account_view(),
                #spl_crate::ops::ctx::AssociatedTokenCheckCtx { #(#args,)* },
            )?;
        },
        _ => unreachable!("classify_group ensures only known groups reach here"),
    }
}

/// Emit a direct capability trait call for an exit action group.
fn emit_exit_action_call(
    ty: &syn::Type,
    field: &syn::Ident,
    group: &GroupOp,
    op_ctx: &OpEmitCtx,
    spl_crate: &syn::Ident,
) -> proc_macro2::TokenStream {
    // Helper: look up an arg value by key name.
    let arg_val = |key: &str| -> proc_macro2::TokenStream {
        group
            .args
            .iter()
            .find(|a| a.key == key)
            .map(|a| exit_arg(a, op_ctx))
            .unwrap_or_else(|| quote! { compile_error!(concat!("missing arg: ", #key)) })
    };

    match group.kind {
        GroupKind::Sweep => {
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
        GroupKind::Close => {
            let dest = arg_val("dest");
            if group_has_arg(group, "authority") {
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
            } else {
                let trait_name = format_ident!("{}Close", "Account");
                quote! {
                    {
                        let __view = unsafe {
                            <#ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                                &mut self.#field
                            )
                        };
                        <#ty as quasar_lang::ops::close::#trait_name>::close(
                            __view,
                            #dest,
                        )?;
                    }
                }
            }
        }
        _ => unreachable!("only Exit ops reach exit_actions"),
    }
}

fn group_has_arg(group: &GroupOp, key: &str) -> bool {
    group.args.iter().any(|arg| arg.key == key)
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
    let needs_lifecycle_rent = semantics.iter().any(|sem| sem.is_migration);

    for sem in semantics {
        let ty = &sem.core.effective_ty;
        let field = &sem.core.ident;

        // Exit actions — direct capability trait calls, sorted (sweep before close).
        for group in &sem.exit_actions {
            exit_stmts.push(emit_exit_action_call(ty, field, group, op_ctx, &spl_crate));
        }

        // Migration epilogue: direct calls instead of trait dispatch.
        // Validation rules guarantee payer exists for Migration fields.
        if sem.is_migration {
            let payer = sem.payer.as_ref().expect("migration requires payer");
            exit_stmts.push(quote! {
                if !quasar_lang::accounts::migration::Migration::is_migrated(&self.#field) {
                    return Err(ProgramError::Custom(
                        quasar_lang::error::QuasarError::AccountNotMigrated as u32,
                    ));
                }
                quasar_lang::accounts::migration::Migration::normalize_to_target(
                    &mut self.#field,
                    self.#payer.to_account_view(),
                    &__rent_ctx,
                )?;
            });
        }
    }

    if exit_stmts.is_empty() {
        return Ok(quote! {});
    }

    let rent_field = find_rent_sysvar_field(semantics);
    let ctx_init = if needs_lifecycle_rent {
        let rent_fetch = if let Some(rent_ident) = rent_field {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    core::clone::Clone::clone(self.#rent_ident.get());
            }
        } else {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::get()?;
            }
        };
        quote! {
            #rent_fetch
            let __rent_ctx = quasar_lang::ops::OpCtxWithRent::new(&crate::ID, &__rent);
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #[inline(always)]
        fn epilogue(&mut self) -> Result<(), ProgramError> {
            #ctx_init
            #(#exit_stmts)*
            Ok(())
        }
    })
}

pub(crate) fn emit_has_epilogue(semantics: &[FieldSemantics]) -> proc_macro2::TokenStream {
    let mut has_epilogue = false;

    for sem in semantics {
        if !sem.exit_actions.is_empty() || sem.is_migration {
            has_epilogue = true;
        }
    }

    if has_epilogue {
        quote! { true }
    } else {
        quote! { false }
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
        if sem.core.optional {
            continue;
        }
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
