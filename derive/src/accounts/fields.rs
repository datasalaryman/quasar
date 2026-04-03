//! Per-field codegen for `#[derive(Accounts)]`.
//!
//! Each field in the accounts struct produces: parsing code (account extraction
//! from the accounts slice), validation code (constraint checks), and PDA seed
//! reconstruction. This module is the largest in the derive crate because each
//! field attribute combination produces distinct codegen paths.

#[path = "fields_support.rs"]
mod support;

use {
    self::support::{
        find_field_by_name, resolve_token_program_addr, resolve_token_program_field,
        validate_field_attrs, DetectedFields, TokenProgramResolution,
    },
    super::{
        attrs::{parse_field_attrs, AccountFieldAttrs},
        field_kind::{debug_checked, debug_guard, strip_ref, FieldFlags, FieldKind},
    },
    crate::helpers::{extract_generic_inner_type, seed_slice_expr_for_parse, strip_generics},
    quote::{format_ident, quote},
    syn::{Expr, ExprLit, Ident, Lit, Type},
};

/// Info for a single `#[account(close = dest)]` field.
pub(super) struct CloseFieldInfo {
    pub field: Ident,
    pub destination: Ident,
    /// For token/mint types, CPI close requires the token program and
    /// authority.
    pub cpi_close: Option<CpiCloseInfo>,
}

pub(super) struct CpiCloseInfo {
    pub token_program: Ident,
    pub authority: Ident,
}

/// Info for a single `#[account(sweep = receiver)]` field.
pub(super) struct SweepFieldInfo {
    pub field: Ident,
    pub receiver: Ident,
    pub mint: Ident,
    pub authority: Ident,
    pub token_program: Ident,
}

pub(super) struct ProcessedFields {
    pub field_constructs: Vec<proc_macro2::TokenStream>,
    pub field_checks: Vec<proc_macro2::TokenStream>,
    pub bump_init_vars: Vec<proc_macro2::TokenStream>,
    pub bump_struct_fields: Vec<proc_macro2::TokenStream>,
    pub bump_struct_inits: Vec<proc_macro2::TokenStream>,
    pub seeds_methods: Vec<proc_macro2::TokenStream>,
    pub seed_addr_captures: Vec<proc_macro2::TokenStream>,
    pub field_attrs: Vec<AccountFieldAttrs>,
    pub init_pda_checks: Vec<proc_macro2::TokenStream>,
    pub init_blocks: Vec<proc_macro2::TokenStream>,
    pub close_fields: Vec<CloseFieldInfo>,
    pub sweep_fields: Vec<SweepFieldInfo>,
    pub needs_rent: bool,
    /// If the struct has a `Sysvar<Rent>` field, its ident. Used to avoid
    /// the `sol_get_rent_sysvar` syscall when a rent account is available.
    pub rent_sysvar_field: Option<Ident>,
}

/// Check if a syn::Type is `u8`.
fn is_type_u8(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.path.is_ident("u8"))
}

pub(super) fn process_fields(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    field_name_strings: &[String],
    instruction_args: &Option<Vec<super::InstructionArg>>,
) -> Result<ProcessedFields, proc_macro::TokenStream> {
    let field_attrs: Vec<AccountFieldAttrs> = fields
        .iter()
        .map(parse_field_attrs)
        .collect::<syn::Result<Vec<_>>>()
        .map_err(|e| -> proc_macro::TokenStream { e.to_compile_error().into() })?;

    // --- PDA bump auto-detection pre-scan ---
    // Count bare-bump PDA fields (seeds present, bump = None/bare).
    // Used to determine if a generic `bump: u8` instruction arg can be auto-bound
    // (only when exactly one bare-bump PDA exists).
    let bare_bump_pda_count = field_attrs
        .iter()
        .filter(|a| a.seeds.is_some() && matches!(a.bump, Some(None)))
        .count();

    // --- Feature flags ---

    let has_any_init = field_attrs.iter().any(|a| a.is_init || a.init_if_needed);
    let has_any_ata_init = field_attrs
        .iter()
        .any(|a| (a.is_init || a.init_if_needed) && a.associated_token_mint.is_some());
    let has_any_realloc = field_attrs.iter().any(|a| a.realloc.is_some());
    let has_any_metadata_init = field_attrs
        .iter()
        .any(|a| (a.is_init || a.init_if_needed) && a.metadata_name.is_some());
    let has_any_master_edition_init = field_attrs
        .iter()
        .any(|a| (a.is_init || a.init_if_needed) && a.master_edition_max_supply.is_some());

    // --- Auto-detect fields (single pass over all fields) ---

    let detected = DetectedFields::detect(fields, &field_attrs);

    // --- Validate required fields per feature ---

    let system_program_field = if has_any_init {
        Some(DetectedFields::require(
            detected.system_program,
            "#[account(init)] requires a `Program<System>` field in the accounts struct",
        )?)
    } else {
        None
    };

    let payer_field = if has_any_init {
        Some(DetectedFields::require(
            detected.payer,
            "#[account(init)] requires a `payer` field or explicit `payer = field` attribute",
        )?)
    } else {
        None
    };

    let realloc_payer_field = if has_any_realloc {
        Some(DetectedFields::require(
            detected.realloc_payer,
            "#[account(realloc)] requires a `payer` field, `realloc::payer = field`, or `payer = \
             field` attribute",
        )?)
    } else {
        None
    };

    // Validate payer writability: init and realloc payers must be mutable.
    for (label, payer) in [("init", &payer_field), ("realloc", &realloc_payer_field)] {
        if let Some(payer_ident) = payer {
            let writable = fields
                .iter()
                .zip(field_attrs.iter())
                .find(|(f, _)| f.ident.as_ref() == Some(payer_ident))
                .map(|(f, attrs)| {
                    let eff = extract_generic_inner_type(&f.ty, "Option").unwrap_or(&f.ty);
                    attrs.is_mut || matches!(eff, Type::Reference(r) if r.mutability.is_some())
                })
                .unwrap_or(false);
            if !writable {
                return Err(syn::Error::new_spanned(
                    payer_ident,
                    format!(
                        "`{}` payer `{}` must be `&mut` or `#[account(mut)]`",
                        label, payer_ident
                    ),
                )
                .to_compile_error()
                .into());
            }
        }
    }

    let ata_program_field = if has_any_ata_init {
        Some(DetectedFields::require(
            detected.associated_token_program,
            "#[account(init, associated_token::...)] requires an `AssociatedTokenProgram` field",
        )?)
    } else {
        None
    };

    let metadata_account_field = if has_any_metadata_init {
        let field = detected
            .metadata_account
            .or_else(|| find_field_by_name(fields, "metadata"));
        Some(DetectedFields::require(
            field,
            "`metadata::*` requires a field of type `Account<MetadataAccount>` or a field named \
             `metadata`",
        )?)
    } else {
        None
    };

    let master_edition_account_field = if has_any_master_edition_init {
        let field = detected
            .master_edition_account
            .or_else(|| find_field_by_name(fields, "master_edition"))
            .or_else(|| find_field_by_name(fields, "edition"));
        Some(DetectedFields::require(
            field,
            "`master_edition::*` requires a field of type `Account<MasterEditionAccount>` or a \
             field named `master_edition`/`edition`",
        )?)
    } else {
        None
    };

    let metadata_program_field = if has_any_metadata_init || has_any_master_edition_init {
        Some(DetectedFields::require(
            detected.metadata_program,
            "`metadata::*` / `master_edition::*` requires a `MetadataProgram` field",
        )?)
    } else {
        None
    };

    let mint_authority_field = if has_any_metadata_init || has_any_master_edition_init {
        Some(DetectedFields::require(
            detected.mint_authority,
            "`metadata::*` / `master_edition::*` requires a `mint_authority` or `authority` field",
        )?)
    } else {
        None
    };

    let update_authority_field = if has_any_metadata_init || has_any_master_edition_init {
        Some(
            detected
                .update_authority
                .expect("update_authority field must be present for metadata/master_edition init"),
        )
    } else {
        None
    };

    let rent_field = if has_any_metadata_init || has_any_master_edition_init {
        Some(DetectedFields::require(
            detected.rent_sysvar,
            "`metadata::*` / `master_edition::*` requires a `Sysvar<Rent>` field",
        )?)
    } else {
        None
    };

    let mut field_constructs: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut field_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_init_vars: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_struct_fields: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut bump_struct_inits: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut seeds_methods: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut seed_addr_captures: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut init_pda_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut init_blocks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut close_fields: Vec<CloseFieldInfo> = Vec::new();
    let mut sweep_fields: Vec<SweepFieldInfo> = Vec::new();
    let mut needs_rent = false;

    for (field, attrs) in fields.iter().zip(field_attrs.iter()) {
        let field_name = field
            .ident
            .as_ref()
            .expect("account field must have an identifier");

        let is_optional = extract_generic_inner_type(&field.ty, "Option").is_some();
        let effective_ty = extract_generic_inner_type(&field.ty, "Option").unwrap_or(&field.ty);
        let is_ref_mut = matches!(effective_ty, Type::Reference(r) if r.mutability.is_some());
        let underlying_ty = strip_ref(effective_ty);
        let kind = FieldKind::classify(underlying_ty);
        let flags = FieldFlags::compute(&kind, attrs, is_ref_mut);
        let is_dynamic = kind.is_dynamic();

        validate_field_attrs(field, field_name, attrs, &kind, &flags)?;

        let token_program_for_token = if attrs.token_mint.is_some()
            || attrs.sweep.is_some()
            || (attrs.close.is_some() && kind.is_token_account())
        {
            let requires_runtime_field = attrs.is_init
                || attrs.init_if_needed
                || attrs.sweep.is_some()
                || (attrs.close.is_some() && kind.is_token_account())
                || matches!(kind, FieldKind::InterfaceAccount { .. });
            resolve_token_program_field(
                fields,
                &detected,
                field_name,
                attrs.token_token_program.as_ref(),
                "token::token_program",
                "`token::*` on this field",
                if requires_runtime_field {
                    TokenProgramResolution::FallbackRequired
                } else {
                    TokenProgramResolution::ExplicitOnly
                },
            )?
        } else {
            None
        };

        let token_program_for_mint =
            if attrs.mint_decimals.is_some() || attrs.master_edition_max_supply.is_some() {
                let requires_runtime_field = attrs.is_init
                    || attrs.init_if_needed
                    || attrs.master_edition_max_supply.is_some()
                    || matches!(kind, FieldKind::InterfaceAccount { .. });
                resolve_token_program_field(
                    fields,
                    &detected,
                    field_name,
                    attrs.mint_token_program.as_ref(),
                    "mint::token_program",
                    "`mint::*` / `master_edition::*` on this field",
                    if requires_runtime_field {
                        TokenProgramResolution::FallbackRequired
                    } else {
                        TokenProgramResolution::ExplicitOnly
                    },
                )?
            } else {
                None
            };

        let token_program_for_ata = if attrs.associated_token_mint.is_some() {
            let requires_runtime_field = attrs.is_init
                || attrs.init_if_needed
                || matches!(kind, FieldKind::InterfaceAccount { .. });
            resolve_token_program_field(
                fields,
                &detected,
                field_name,
                attrs.associated_token_token_program.as_ref(),
                "associated_token::token_program",
                "`associated_token::*` on this field",
                if requires_runtime_field {
                    TokenProgramResolution::FallbackRequired
                } else {
                    TokenProgramResolution::ExplicitOnly
                },
            )?
        } else {
            None
        };

        // Generate type-specific validation (owner, discriminator, address).
        // Flags are already validated via u32 header in parse_accounts.
        //
        // Skip the generic Account<T>/InterfaceAccount<T> owner + data_len
        // checks when a more specific validation will run for the same field:
        //
        //   1. init / init_if_needed — the init block's inline validation covers owner,
        //      data_len, is_initialized, and field-specific checks. Generic Account<T>
        //      init_if_needed also validates inline (owner + discriminator in the
        //      else-branch).
        //
        //   2. Non-init fields with token/mint/ATA attrs — `validate_token_account`,
        //      `validate_mint`, or `validate_ata` already checks owner + data_len as
        //      their first two operations. Running CheckOwner + AccountCheck beforehand
        //      is pure redundancy (~60 CU per field).
        //
        //   3. Dynamic Account<T<'info>> fields — `T::from_account_view()` in the field
        //      construct already calls check_owner + AccountCheck::check
        //      (dynamic.rs:421-422). Running them again in field_checks is redundant
        //      (~50-80 CU per dynamic field).
        let has_validate_call = attrs.token_mint.is_some()
            || attrs.associated_token_mint.is_some()
            || attrs.mint_decimals.is_some();
        let skip_mut_checks = attrs.is_init
            || (attrs.init_if_needed
                && matches!(
                    kind,
                    FieldKind::Account { .. } | FieldKind::InterfaceAccount { .. }
                ))
            || (has_validate_call
                && matches!(
                    kind,
                    FieldKind::Account { .. } | FieldKind::InterfaceAccount { .. }
                ))
            || (is_dynamic && matches!(kind, FieldKind::Account { .. }));
        let mut this_field_checks: Vec<proc_macro2::TokenStream> = Vec::new();

        match &kind {
            FieldKind::Account { inner_ty } => {
                if !skip_mut_checks {
                    let field_name_str = field_name.to_string();
                    let owner = debug_checked(
                        &field_name_str,
                        quote! { <#inner_ty as quasar_lang::traits::CheckOwner>::check_owner(#field_name.to_account_view()) },
                        "Owner check failed for account '{}'",
                    );
                    let disc = debug_checked(
                        &field_name_str,
                        quote! { <#inner_ty as quasar_lang::traits::AccountCheck>::check(#field_name.to_account_view()) },
                        "Discriminator check failed for account '{}': data may be uninitialized \
                         or corrupted",
                    );
                    this_field_checks.push(quote! {
                        #owner
                        #disc
                    });
                }
            }
            FieldKind::InterfaceAccount { inner_ty } => {
                if !skip_mut_checks {
                    let field_name_str = field_name.to_string();
                    let disc = debug_checked(
                        &field_name_str,
                        quote! { <#inner_ty as quasar_lang::traits::AccountCheck>::check(#field_name.to_account_view()) },
                        "Account check failed for interface account '{}': data may be \
                         uninitialized or corrupted",
                    );
                    let owner_guard = debug_guard(
                        quote! {
                            {
                                let __owner = #field_name.to_account_view().owner();
                                !quasar_lang::keys_eq(__owner, &quasar_spl::SPL_TOKEN_ID)
                                    && !quasar_lang::keys_eq(__owner, &quasar_spl::TOKEN_2022_ID)
                            }
                        },
                        quote! { ::alloc::format!(
                            "Owner check failed for interface account '{}': not owned by SPL Token or Token-2022",
                            #field_name_str
                        ) },
                        quote! { ProgramError::IllegalOwner },
                    );
                    this_field_checks.push(quote! {
                        #owner_guard
                        #disc
                    });
                }
            }
            FieldKind::Sysvar { inner_ty } => {
                let field_name_str = field_name.to_string();
                this_field_checks.push(debug_guard(
                    quote! { !quasar_lang::keys_eq(#field_name.to_account_view().address(), &<#inner_ty as quasar_lang::sysvars::Sysvar>::ID) },
                    quote! { ::alloc::format!(
                        "Incorrect sysvar address for account '{}': expected {}, got {}",
                        #field_name_str,
                        <#inner_ty as quasar_lang::sysvars::Sysvar>::ID,
                        #field_name.to_account_view().address()
                    ) },
                    quote! { ProgramError::IncorrectProgramId },
                ));
            }
            FieldKind::Program { inner_ty } => {
                let field_name_str = field_name.to_string();
                this_field_checks.push(debug_guard(
                    quote! { !quasar_lang::keys_eq(#field_name.to_account_view().address(), &<#inner_ty as quasar_lang::traits::Id>::ID) },
                    quote! { ::alloc::format!(
                        "Incorrect program ID for account '{}': expected {}, got {}",
                        #field_name_str,
                        <#inner_ty as quasar_lang::traits::Id>::ID,
                        #field_name.to_account_view().address()
                    ) },
                    quote! { ProgramError::IncorrectProgramId },
                ));
            }
            FieldKind::Interface { inner_ty } => {
                let field_name_str = field_name.to_string();
                this_field_checks.push(debug_guard(
                    quote! { !<#inner_ty as quasar_lang::traits::ProgramInterface>::matches(#field_name.to_account_view().address()) },
                    quote! { ::alloc::format!(
                        "Program interface mismatch for account '{}': address {} does not match any allowed programs",
                        #field_name_str,
                        #field_name.to_account_view().address()
                    ) },
                    quote! { ProgramError::IncorrectProgramId },
                ));
            }
            FieldKind::SystemAccount => {
                let field_name_str = field_name.to_string();
                let base_type = strip_generics(underlying_ty);
                let owner = debug_checked(
                    &field_name_str,
                    quote! { <#base_type as quasar_lang::checks::Owner>::check(#field_name.to_account_view()) },
                    "Owner check failed for account '{}': not owned by system program",
                );
                this_field_checks.push(owner);
            }
            FieldKind::Signer | FieldKind::Other => {}
        }

        // Field construction — flags already validated via header check
        let construct = |expr: proc_macro2::TokenStream| {
            if is_optional {
                quote! { #field_name: if quasar_lang::keys_eq(#field_name.address(), __program_id) { None } else { Some(#expr) } }
            } else {
                quote! { #field_name: #expr }
            }
        };

        if is_dynamic {
            // Dynamic accounts (Account<T<'info>>): fallible parse from account view
            if let FieldKind::Account { inner_ty } = &kind {
                let inner_base = strip_generics(inner_ty);
                field_constructs.push(construct(
                    quote! { #inner_base::from_account_view(#field_name)? },
                ));
            } else {
                let base_type = strip_generics(effective_ty);
                field_constructs
                    .push(quote! { #field_name: #base_type::from_account_view(#field_name)? });
            }
        } else if let Type::Reference(type_ref) = effective_ty {
            // &T or &mut T: zero-copy from account view
            let base_type = strip_generics(&type_ref.elem);
            let expr = if type_ref.mutability.is_some() {
                quote! { unsafe { #base_type::from_account_view_unchecked_mut(#field_name) } }
            } else {
                quote! { unsafe { #base_type::from_account_view_unchecked(#field_name) } }
            };
            field_constructs.push(construct(expr));
        } else {
            // Bare types (Signer, SystemAccount, UncheckedAccount, etc.)
            let base_type = strip_generics(effective_ty);
            field_constructs.push(construct(
                quote! { unsafe { #base_type::from_account_view_unchecked(#field_name) } },
            ));
        }

        let field_name_str = field_name.to_string();
        for (target, custom_error) in &attrs.has_ones {
            let error = match custom_error {
                Some(err) => quote! { #err.into() },
                None => quote! { QuasarError::HasOneMismatch.into() },
            };
            let target_str = target.to_string();
            this_field_checks.push(debug_guard(
                quote! { !quasar_lang::keys_eq(&#field_name.#target, #target.to_account_view().address()) },
                quote! { ::alloc::format!(
                    "has_one mismatch: '{}.{}' does not match account '{}'",
                    #field_name_str, #target_str, #target_str,
                ) },
                quote! { #error },
            ));
        }

        for (expr, custom_error) in &attrs.constraints {
            let error = match custom_error {
                Some(err) => quote! { #err.into() },
                None => quote! { QuasarError::ConstraintViolation.into() },
            };
            let expr_str = quote!(#expr).to_string();
            this_field_checks.push(debug_guard(
                quote! { !(#expr) },
                quote! { ::alloc::format!(
                    "Constraint violated on '{}': `{}`",
                    #field_name_str, #expr_str,
                ) },
                quote! { #error },
            ));
        }

        if let Some((addr_expr, custom_error)) = &attrs.address {
            let error = match custom_error {
                Some(err) => quote! { #err.into() },
                None => quote! { QuasarError::AddressMismatch.into() },
            };
            this_field_checks.push(debug_guard(
                quote! { !quasar_lang::keys_eq(#field_name.to_account_view().address(), &#addr_expr) },
                quote! { ::alloc::format!(
                    "Address mismatch on '{}': got {}",
                    #field_name_str,
                    #field_name.to_account_view().address(),
                ) },
                quote! { #error },
            ));
        }

        // --- Close field tracking ---

        if let Some(dest) = &attrs.close {
            let cpi_close = if kind.is_token_account() {
                // Token accounts require CPI close via the token program.
                // Resolve the authority from token::authority (or associated_token::authority).
                let authority = attrs
                    .token_authority
                    .clone()
                    .or_else(|| attrs.associated_token_authority.clone())
                    .ok_or_else(|| -> proc_macro::TokenStream {
                        syn::Error::new_spanned(
                            field_name,
                            "#[account(close)] on token account types requires `token::authority`",
                        )
                        .to_compile_error()
                        .into()
                    })?;
                let tp_field: Ident = token_program_for_token
                    .cloned()
                    .expect("token close requires a resolved token program field");
                Some(CpiCloseInfo {
                    token_program: tp_field,
                    authority,
                })
            } else {
                None
            };
            close_fields.push(CloseFieldInfo {
                field: field_name.clone(),
                destination: dest.clone(),
                cpi_close,
            });
        }

        // --- Sweep field tracking ---

        if let Some(receiver) = &attrs.sweep {
            // Validate the sweep target field exists and is a token account type.
            let receiver_field = fields
                .iter()
                .find(|f| f.ident.as_ref() == Some(receiver))
                .ok_or_else(|| -> proc_macro::TokenStream {
                    syn::Error::new_spanned(
                        receiver,
                        format!("sweep target `{}` not found in accounts struct", receiver),
                    )
                    .to_compile_error()
                    .into()
                })?;
            if !FieldKind::classify(strip_ref(&receiver_field.ty)).is_token_account() {
                return Err(syn::Error::new_spanned(
                    receiver,
                    "sweep target must be a token account (Account<Token>, Account<Token2022>, or \
                     InterfaceAccount<Token>)",
                )
                .to_compile_error()
                .into());
            }
            // Validate target is mutable.
            let target_is_mut = matches!(&receiver_field.ty, Type::Reference(r) if r.mutability.is_some())
                || field_attrs
                    .iter()
                    .zip(fields.iter())
                    .any(|(a, f)| f.ident.as_ref() == Some(receiver) && a.is_mut);
            if !target_is_mut {
                return Err(syn::Error::new_spanned(
                    receiver,
                    "sweep target must be mutable (`&mut` or `#[account(mut)]`)",
                )
                .to_compile_error()
                .into());
            }
            // Validate token::authority is a Signer.
            if let Some(auth_name) = &attrs.token_authority {
                let auth_field = fields.iter().find(|f| f.ident.as_ref() == Some(auth_name));
                if let Some(af) = auth_field {
                    if !matches!(FieldKind::classify(strip_ref(&af.ty)), FieldKind::Signer) {
                        return Err(syn::Error::new_spanned(
                            auth_name,
                            "sweep requires `token::authority` to be a Signer (it must sign the \
                             transfer_checked CPI)",
                        )
                        .to_compile_error()
                        .into());
                    }
                }
            }

            let mint = attrs
                .token_mint
                .clone()
                .expect("token_mint must be set when sweep is configured");
            let authority = attrs
                .token_authority
                .clone()
                .expect("token_authority must be set when sweep is configured");
            let tp_field = token_program_for_token
                .cloned()
                .expect("token_program field must be present when sweep is configured");

            sweep_fields.push(SweepFieldInfo {
                field: field_name.clone(),
                receiver: receiver.clone(),
                mint,
                authority,
                token_program: tp_field,
            });
        }

        // --- PDA seeds + init code generation ---

        let is_init_field = attrs.is_init || attrs.init_if_needed;

        if let Some(seed_exprs) = &attrs.seeds {
            let bump_var = format_ident!("__bumps_{}", field_name);

            bump_init_vars.push(quote! { let mut #bump_var: u8 = 0; });
            bump_struct_fields.push(quote! { pub #field_name: u8 });
            bump_struct_inits.push(quote! { #field_name: #bump_var });

            let bump_arr_field = format_ident!("__{}_bump", field_name);
            bump_struct_fields.push(quote! { #bump_arr_field: [u8; 1] });
            bump_struct_inits.push(quote! { #bump_arr_field: [#bump_var] });

            let seed_slices: Vec<proc_macro2::TokenStream> = seed_exprs
                .iter()
                .map(|expr| seed_slice_expr_for_parse(expr, field_name_strings))
                .collect();

            // Solana enforces MAX_SEEDS = 16 at runtime; we catch it here at compile time
            // since seed count is always statically known from the attribute.
            // (15 explicit seeds + 1 bump = 16 total)
            if seed_slices.len() > 15 {
                return Err(syn::Error::new_spanned(
                    field_name,
                    format!(
                        "`{}` exceeds Solana's PDA seed limit: {} seeds provided, max is 16 \
                         including bump",
                        field_name,
                        seed_slices.len()
                    ),
                )
                .to_compile_error()
                .into());
            }

            let seed_idents: Vec<Ident> = seed_slices
                .iter()
                .enumerate()
                .map(|(idx, _)| format_ident!("__seed_{}_{}", field_name, idx))
                .collect();

            let seed_len_checks: Vec<proc_macro2::TokenStream> = seed_idents
                .iter()
                .zip(seed_slices.iter())
                .zip(seed_exprs.iter())
                .map(|((ident, seed), expr)| {
                    match expr {
                        // static byte string (b"quasar") — compile time check
                        Expr::Lit(ExprLit {
                            lit: Lit::ByteStr(b),
                            ..
                        }) => {
                            let len = b.value().len();
                            if len > 32 {
                                return syn::Error::new_spanned(
                                    expr,
                                    format!(
                                        "seed b\"{}\" is {} bytes, exceeds MAX_SEED_LEN of 32",
                                        String::from_utf8_lossy(&b.value()),
                                        len
                                    ),
                                )
                                .to_compile_error();
                            }
                            quote! { let #ident: &[u8] = #seed; }
                        }

                        // dynamic — runtime check
                        _ => quote! {
                            let #ident: &[u8] = #seed;
                            if #ident.len() > 32 {
                                return Err(QuasarError::InvalidSeeds.into());
                            }
                        },
                    }
                })
                .collect();
            // Choose target: init_pda_checks for init fields, this_field_checks for others
            let target_checks = if is_init_field {
                &mut init_pda_checks
            } else {
                &mut this_field_checks
            };

            // Init fields are still raw &AccountView at PDA check time;
            // non-init fields are typed wrappers (rebound via let Self { ref ... } =
            // result)
            let addr_access = if is_init_field {
                quote! { *#field_name.address() }
            } else {
                quote! { *#field_name.to_account_view().address() }
            };

            match &attrs.bump {
                Some(Some(bump_expr)) => {
                    let check = quote! {
                        {
                            #(#seed_len_checks)*
                            let __bump_val: u8 = #bump_expr;
                            let __bump_ref: &[u8] = &[__bump_val];
                            let __pda_seeds = [#(#seed_idents,)* __bump_ref];
                            quasar_lang::pda::verify_program_address(&__pda_seeds, __program_id, &#addr_access)
                                .map_err(|__e| {
                                    #[cfg(feature = "debug")]
                                    quasar_lang::prelude::log(concat!(
                                        "Account '", stringify!(#field_name),
                                        "': PDA verification failed"
                                    ));
                                    __e
                                })?;
                            #bump_var = __bump_val;
                        }
                    };
                    target_checks.push(check);
                }
                Some(None) => {
                    // --- PDA bump auto-detection ---
                    // Priority: 1) instruction arg match  2) inner account stored bump  3) find
                    let field_bump_name = format!("{}_bump", field_name);

                    // Try matching instruction arg: {field}_bump: u8 or single-PDA bump: u8
                    let ix_arg_match = instruction_args.as_ref().and_then(|args| {
                        args.iter().find(|a| {
                            if !is_type_u8(&a.ty) {
                                return false;
                            }
                            let name = a.name.to_string();
                            if name == field_bump_name {
                                return true; // Rule 2: {field_name}_bump: u8
                            }
                            if name == "bump" && bare_bump_pda_count == 1 {
                                return true; // Rule 3: single bare-bump PDA +
                                             // bump: u8
                            }
                            false
                        })
                    });

                    let check = if let Some(arg) = ix_arg_match {
                        // Auto-bind: instruction arg → verify_program_address
                        let arg_ident = &arg.name;
                        quote! {
                            {
                                #(#seed_len_checks)*
                                let __bump_val: u8 = #arg_ident;
                                let __bump_ref: &[u8] = &[__bump_val];
                                let __pda_seeds = [#(#seed_idents,)* __bump_ref];
                                quasar_lang::pda::verify_program_address(&__pda_seeds, __program_id, &#addr_access)
                                    .map_err(|__e| {
                                        #[cfg(feature = "debug")]
                                        quasar_lang::prelude::log(concat!(
                                            "Account '", stringify!(#field_name),
                                            "': PDA verification failed"
                                        ));
                                        __e
                                    })?;
                                #bump_var = __bump_val;
                            }
                        }
                    } else if !is_init_field {
                        // Try inner account's BUMP_OFFSET (non-init only — init accounts don't have
                        // data yet). Extract inner type T from Account<T>
                        // for BUMP_OFFSET lookup.
                        if let FieldKind::Account { inner_ty } = &kind {
                            // Account<T>: use BUMP_OFFSET if available, else find
                            let view_access = quote! { #field_name.to_account_view() };
                            quote! {
                                {
                                    #(#seed_len_checks)*
                                    if let Some(__offset) = <#inner_ty as Discriminator>::BUMP_OFFSET {
                                        if quasar_lang::utils::hint::unlikely(__offset >= #view_access.data_len()) {
                                            #[cfg(feature = "debug")]
                                            quasar_lang::prelude::log(concat!(
                                                "BUMP_OFFSET out of bounds for account '",
                                                stringify!(#field_name), "'"
                                            ));
                                            return Err(ProgramError::AccountDataTooSmall);
                                        }
                                        let __bump_val: u8 = unsafe { *#view_access.data_ptr().add(__offset) };
                                        let __bump_ref: &[u8] = &[__bump_val];
                                        let __pda_seeds = [#(#seed_idents,)* __bump_ref];
                                        quasar_lang::pda::verify_program_address(&__pda_seeds, __program_id, &#addr_access)
                                            .map_err(|__e| {
                                                #[cfg(feature = "debug")]
                                                quasar_lang::prelude::log(concat!(
                                                    "Account '", stringify!(#field_name),
                                                    "': PDA verification failed"
                                                ));
                                                __e
                                            })?;
                                        #bump_var = __bump_val;
                                    } else {
                                        let __pda_seeds = [#(#seed_idents),*];
                                        let (__expected, __bump) = quasar_lang::pda::based_try_find_program_address(&__pda_seeds, __program_id)?;
                                        if #addr_access != __expected {
                                            #[cfg(feature = "debug")]
                                            quasar_lang::prelude::log(concat!(
                                                "Account '", stringify!(#field_name),
                                                "': PDA verification failed"
                                            ));
                                            return Err(QuasarError::InvalidPda.into());
                                        }
                                        #bump_var = __bump;
                                    }
                                }
                            }
                        } else {
                            // Non-Account type (UncheckedAccount, etc.): find
                            quote! {
                                {
                                    #(#seed_len_checks)*
                                    let __pda_seeds = [#(#seed_idents),*];
                                    let (__expected, __bump) = quasar_lang::pda::based_try_find_program_address(&__pda_seeds, __program_id)?;
                                    if #addr_access != __expected {
                                        #[cfg(feature = "debug")]
                                        quasar_lang::prelude::log(concat!(
                                            "Account '", stringify!(#field_name),
                                            "': PDA verification failed"
                                        ));
                                        return Err(QuasarError::InvalidPda.into());
                                    }
                                    #bump_var = __bump;
                                }
                            }
                        }
                    } else {
                        // Init field: no stored bump yet, use find
                        quote! {
                            {
                                #(#seed_len_checks)*
                                let __pda_seeds = [#(#seed_idents),*];
                                let (__expected, __bump) = quasar_lang::pda::based_try_find_program_address(&__pda_seeds, __program_id)?;
                                if #addr_access != __expected {
                                    #[cfg(feature = "debug")]
                                    quasar_lang::prelude::log(concat!(
                                        "Account '", stringify!(#field_name),
                                        "': PDA verification failed"
                                    ));
                                    return Err(QuasarError::InvalidPda.into());
                                }
                                #bump_var = __bump;
                            }
                        }
                    };

                    target_checks.push(check);
                }
                None => {
                    return Err(syn::Error::new_spanned(
                        field_name,
                        "#[account(seeds = [...])] requires a `bump` or `bump = expr` directive",
                    )
                    .to_compile_error()
                    .into());
                }
            }

            let method_name = format_ident!("{}_seeds", field_name);
            let seed_count = seed_exprs.len() + 1;
            let mut seed_elements: Vec<proc_macro2::TokenStream> = Vec::new();

            for expr in seed_exprs {
                if let Expr::Path(ep) = expr {
                    if ep.qself.is_none() && ep.path.segments.len() == 1 {
                        let ident = &ep.path.segments[0].ident;
                        if field_name_strings.contains(&ident.to_string()) {
                            let addr_field = format_ident!("__seed_{}_{}", field_name, ident);
                            let capture_var = format_ident!("__seed_addr_{}_{}", field_name, ident);

                            seed_addr_captures.push(quote! {
                                let #capture_var = *#ident.address();
                            });
                            bump_struct_fields.push(quote! { #addr_field: Address });
                            bump_struct_inits.push(quote! { #addr_field: #capture_var });

                            seed_elements.push(
                                quote! { quasar_lang::cpi::Seed::from(self.#addr_field.as_ref()) },
                            );
                            continue;
                        }
                    }
                }
                seed_elements.push(quote! { quasar_lang::cpi::Seed::from((#expr) as &[u8]) });
            }

            seed_elements
                .push(quote! { quasar_lang::cpi::Seed::from(&self.#bump_arr_field as &[u8]) });

            seeds_methods.push(quote! {
                #[inline(always)]
                pub fn #method_name(&self) -> [quasar_lang::cpi::Seed<'_>; #seed_count] {
                    [#(#seed_elements),*]
                }
            });
        }

        // --- Init code generation ---

        if is_init_field {
            let init_ctx = super::init::InitContext {
                payer: payer_field.expect("payer field must be present for init"),
                system_program: system_program_field
                    .expect("system_program field must be present for init"),
                token_program: if attrs.token_mint.is_some() || attrs.sweep.is_some() {
                    token_program_for_token
                } else if attrs.associated_token_mint.is_some() {
                    token_program_for_ata
                } else if attrs.mint_decimals.is_some() || attrs.master_edition_max_supply.is_some()
                {
                    token_program_for_mint
                } else {
                    None
                },
                ata_program: ata_program_field,
                metadata_account: metadata_account_field,
                master_edition_account: master_edition_account_field,
                metadata_program: metadata_program_field,
                mint_authority: mint_authority_field,
                update_authority: update_authority_field,
                rent: rent_field,
                field_name_strings,
            };

            if let Some(result) =
                super::init::gen_init_block(field_name, attrs, effective_ty, &init_ctx)?
            {
                init_blocks.push(result.tokens);
                needs_rent |= result.uses_rent;
            }

            // Metadata CPI (does not use __shared_rent)
            if let Some(block) = super::init::gen_metadata_init(field_name, attrs, &init_ctx) {
                init_blocks.push(block);
            }

            // Master edition CPI (does not use __shared_rent)
            if let Some(block) = super::init::gen_master_edition_init(field_name, attrs, &init_ctx)
            {
                init_blocks.push(block);
            }
        }

        // --- Non-init ATA address validation ---

        if let (false, Some(mint_field), Some(auth_field)) = (
            is_init_field,
            attrs.associated_token_mint.as_ref(),
            attrs.associated_token_authority.as_ref(),
        ) {
            let token_program_addr = if let Some(tp) = &attrs.associated_token_token_program {
                quote! { #tp.to_account_view().address() }
            } else {
                resolve_token_program_addr(effective_ty, token_program_for_ata)
            };

            this_field_checks.push(quote! {
                quasar_spl::validate_ata(
                    #field_name.to_account_view(),
                    #auth_field.to_account_view().address(),
                    #mint_field.to_account_view().address(),
                    #token_program_addr,
                )?;
            });
        }

        // --- Non-init token account validation ---

        if let (false, Some(mint_field), Some(auth_field)) = (
            is_init_field,
            attrs.token_mint.as_ref(),
            attrs.token_authority.as_ref(),
        ) {
            let token_program_addr =
                resolve_token_program_addr(effective_ty, token_program_for_token);
            this_field_checks.push(quote! {
                quasar_spl::validate_token_account(
                    #field_name.to_account_view(),
                    #mint_field.to_account_view().address(),
                    #auth_field.to_account_view().address(),
                    #token_program_addr,
                )?;
            });
        }

        // --- Non-init mint validation ---

        if let (false, Some(decimals_expr), Some(auth_field)) = (
            is_init_field,
            attrs.mint_decimals.as_ref(),
            attrs.mint_init_authority.as_ref(),
        ) {
            let token_program_addr =
                resolve_token_program_addr(effective_ty, token_program_for_mint);
            let freeze_expr = if let Some(freeze_field) = &attrs.mint_freeze_authority {
                quote! { Some(#freeze_field.to_account_view().address()) }
            } else {
                quote! { None }
            };
            this_field_checks.push(quote! {
                quasar_spl::validate_mint(
                    #field_name.to_account_view(),
                    #auth_field.to_account_view().address(),
                    (#decimals_expr) as u8,
                    #freeze_expr,
                    #token_program_addr,
                )?;
            });
        }

        // --- Realloc code generation ---

        if let Some(realloc_expr) = &attrs.realloc {
            let realloc_pay = realloc_payer_field.expect("payer field must be present for realloc");
            needs_rent = true;

            init_blocks.push(quote! {
                {
                    let __realloc_space = (#realloc_expr) as usize;
                    quasar_lang::accounts::realloc_account(
                        #field_name, __realloc_space, #realloc_pay, Some(&__shared_rent)
                    )?;
                }
            });
        }

        // --- Flush per-field checks ---
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
    }

    Ok(ProcessedFields {
        field_constructs,
        field_checks,
        bump_init_vars,
        bump_struct_fields,
        bump_struct_inits,
        seeds_methods,
        seed_addr_captures,
        field_attrs,
        init_pda_checks,
        init_blocks,
        close_fields,
        sweep_fields,
        needs_rent,
        rent_sysvar_field: detected.rent_sysvar.cloned(),
    })
}

/// Determine which NODUP constant to use for a field.
/// Returns the constant name as a string for code generation.
pub(super) fn determine_nodup_constant(
    field: &syn::Field,
    attrs: &super::attrs::AccountFieldAttrs,
    is_ref_mut: bool,
) -> &'static str {
    // No Option stripping — only called for non-optional non-dup fields
    let ty = strip_ref(&field.ty);
    let kind = FieldKind::classify(ty);
    FieldFlags::compute(&kind, attrs, is_ref_mut).nodup_constant()
}

/// Compute the expected u32 header value for a field based on its attributes
/// and type.
///
/// Returns a u32 in little-endian byte order:
/// - Byte 0: borrow_state (always 0xFF for no-dup)
/// - Byte 1: is_signer (1 if required, 0 otherwise)
/// - Byte 2: is_writable (1 if required, 0 otherwise)
/// - Byte 3: executable (1 if required, 0 otherwise)
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
