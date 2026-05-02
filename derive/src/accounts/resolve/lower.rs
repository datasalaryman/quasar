//! Lower parsed directives into FieldSemantics with phase-tagged groups.

use {
    super::{
        super::syntax::{
            attrs::{validate_op_arg, Directive},
            parse_field_attrs,
        },
        rules::validate_semantics,
        FieldCore, FieldKind, FieldSemantics, GroupArg, GroupKind, GroupOp, InitDirective, OpKind,
    },
    crate::helpers::{extract_generic_inner_type, is_composite_type},
    quote::format_ident,
    syn::Type,
};

pub(super) fn lower_semantics(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    _instruction_args: &Option<Vec<crate::accounts::InstructionArg>>,
) -> syn::Result<Vec<FieldSemantics>> {
    let parsed: Vec<(syn::Field, Vec<Directive>)> = fields
        .iter()
        .map(|field| Ok((field.clone(), parse_field_attrs(field)?)))
        .collect::<syn::Result<_>>()?;

    let cores: Vec<FieldCore> = parsed
        .iter()
        .map(|(field, directives)| lower_core(field, directives))
        .collect();

    let semantics: Vec<FieldSemantics> = parsed
        .into_iter()
        .zip(cores)
        .map(|((_, directives), core)| {
            let is_migration = detect_migration(&core.effective_ty);
            let mut sem = FieldSemantics {
                core,
                init: None,
                payer: None,
                address: None,
                realloc: None,
                groups: Vec::new(),
                constraints: Vec::new(),
                init_contributors: Vec::new(),
                exit_actions: Vec::new(),
                user_checks: Vec::new(),
                is_migration,
            };
            lower_directives(&mut sem, directives)?;
            Ok(sem)
        })
        .collect::<syn::Result<_>>()?;

    // Infer missing args before validation. Order matters:
    // 1. Payer (before validate_semantics checks init requires payer)
    // 2. Program args (before validate_close_groups checks authority/token_program pairing)
    let mut semantics = semantics;
    resolve_payer(&mut semantics);
    resolve_program_args(&mut semantics)?;

    validate_semantics(&semantics)?;

    // Classify groups into buckets after validation.
    let semantics = semantics
        .into_iter()
        .map(|sem| classify_groups(sem))
        .collect::<syn::Result<_>>()?;

    Ok(semantics)
}

fn lower_core(field: &syn::Field, directives: &[Directive]) -> FieldCore {
    let ty = &field.ty;
    let optional = extract_generic_inner_type(ty, "Option").is_some();
    let after_option = extract_generic_inner_type(ty, "Option")
        .cloned()
        .unwrap_or_else(|| ty.clone());

    let effective_ty = match &after_option {
        Type::Reference(r) => (*r.elem).clone(),
        other => other.clone(),
    };

    let kind = classify_kind(ty);

    let inner_ty = extract_inner_ty(&effective_ty);
    let dynamic = detect_dynamic(&effective_ty, inner_ty.as_ref());

    FieldCore {
        ident: field
            .ident
            .clone()
            .expect("account field must have an identifier"),
        field: field.clone(),
        effective_ty,
        kind,
        inner_ty,
        optional,
        dynamic,
        is_mut: directives
            .iter()
            .any(|d| matches!(d, Directive::Bare(id) if id == "mut")),
        dup: directives
            .iter()
            .any(|d| matches!(d, Directive::Bare(id) if id == "dup")),
    }
}

fn classify_kind(raw_ty: &Type) -> FieldKind {
    if is_composite_type(raw_ty) {
        FieldKind::Composite
    } else {
        FieldKind::Single
    }
}

fn lower_directives(sem: &mut FieldSemantics, directives: Vec<Directive>) -> syn::Result<()> {
    let mut groups = Vec::new();

    for directive in directives {
        match directive {
            Directive::Bare(_) => { /* mut/dup handled in lower_core */ }
            Directive::Init { idempotent } => {
                sem.init = Some(InitDirective { idempotent });
                sem.core.is_mut = true;
            }
            Directive::Payer(ident) => {
                sem.payer = Some(ident);
            }
            Directive::Address(expr, error) => {
                if error.is_some() {
                    // Custom error: use the check path (custom error message).
                    sem.user_checks
                        .push(super::UserCheck::Address { expr, error });
                } else {
                    // No custom error: use AddressVerify (supports seeds + literal).
                    sem.address = Some(expr);
                }
            }
            Directive::Realloc(expr) => {
                sem.realloc = Some(expr);
            }
            Directive::Allow(_) => { /* lint-only, ignored by derive */ }
            Directive::Group(group) => {
                groups.push(group);
            }
            Directive::Check(check) => {
                sem.user_checks.push(check);
            }
        }
    }

    // Validate op-arg grammar on op groups.
    for group in &groups {
        for arg in &group.args {
            validate_op_arg(&arg.key, &arg.value)?;
        }
    }

    sem.groups = groups;

    Ok(())
}

// --- Type classification helpers ---

fn extract_inner_ty(effective_ty: &Type) -> Option<Type> {
    for wrapper in &[
        "Account",
        "InterfaceAccount",
        "Migration",
        "Program",
        "Interface",
        "Sysvar",
    ] {
        if let Some(inner) = extract_generic_inner_type(effective_ty, wrapper) {
            return Some(inner.clone());
        }
    }
    None
}

fn detect_dynamic(effective_ty: &Type, inner_ty: Option<&Type>) -> bool {
    if extract_generic_inner_type(effective_ty, "Account").is_none() {
        return false;
    }
    let Some(inner) = inner_ty else { return false };
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

// --- Migration detection ---

/// Syntactic detection: last path segment is `Migration`.
/// Proc macros cannot resolve type aliases — only direct `Migration<From, To>`
/// paths are supported.
fn detect_migration(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Migration"),
        _ => false,
    }
}

// --- Payer inference ---

/// If a field has `init` but no `payer = ...`, and the struct has a field
/// named `payer`, inject it. Convention-based: the name `payer` is the
/// standard field name for the fee payer in Solana programs.
fn resolve_payer(semantics: &mut [FieldSemantics]) {
    let has_payer_field = semantics
        .iter()
        .any(|sem| sem.core.ident == "payer" && sem.core.kind == FieldKind::Single);

    if !has_payer_field {
        return;
    }

    for sem in semantics.iter_mut() {
        if sem.payer.is_none() {
            let needs_payer =
                sem.init.is_some() || sem.is_migration || sem.realloc.is_some();
            if needs_payer {
                sem.payer = Some(format_ident!("payer"));
            }
        }
    }
}

// --- Program inference ---

/// Program category for inference resolution.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProgramCategory {
    /// Program<TokenProgram> — SPL Token v1 only.
    Token,
    /// Program<Token2022Program> — Token-2022 only.
    Token2022,
    /// Interface<TokenInterface> — polymorphic Token v1 or Token-2022.
    TokenInterface,
    /// Program<SystemProgram>.
    System,
    /// Program<AssociatedTokenProgram>.
    Ata,
}

/// A program field candidate for inference.
struct ProgramCandidate {
    ident: syn::Ident,
    category: ProgramCategory,
}

/// Classify a field's inner type into a program category.
/// Returns `None` for non-program types.
fn classify_program(effective_ty: &Type, optional: bool, dup: bool, kind: FieldKind) -> Option<ProgramCategory> {
    // Exclude optional, dup, and composite fields.
    if optional || dup || kind != FieldKind::Single {
        return None;
    }

    // Check Program<X> wrapper.
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Program") {
        return classify_program_inner(inner);
    }

    // Check Interface<X> wrapper.
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Interface") {
        if let Some(name) = last_segment_name(inner) {
            if name == concat!("Token", "Interface") {
                return Some(ProgramCategory::TokenInterface);
            }
        }
    }

    None
}

/// Classify a Program<X> inner type name.
fn classify_program_inner(inner: &Type) -> Option<ProgramCategory> {
    let name = last_segment_name(inner)?;
    if name == concat!("Token", "Program") {
        Some(ProgramCategory::Token)
    } else if name == concat!("Token2022", "Program") {
        Some(ProgramCategory::Token2022)
    } else if name == concat!("System", "Program") {
        Some(ProgramCategory::System)
    } else if name == concat!("Associated", "Token", "Program") {
        Some(ProgramCategory::Ata)
    } else {
        None
    }
}

/// Get the last segment name from a type path.
fn last_segment_name(ty: &Type) -> Option<String> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

/// Determine which token program categories are compatible with the
/// op-bearing field's account type.
fn compatible_token_categories(effective_ty: &Type) -> &'static [ProgramCategory] {
    // Account<Token> / Account<Mint> → only Program<TokenProgram>
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Account") {
        if let Some(name) = last_segment_name(inner) {
            if name == "Token" || name == "Mint" {
                return &[ProgramCategory::Token];
            }
            if name == "Token2022" || name == "Mint2022" {
                return &[ProgramCategory::Token2022];
            }
        }
    }

    // InterfaceAccount<Token> / InterfaceAccount<Mint> → interface priority
    if extract_generic_inner_type(effective_ty, "InterfaceAccount").is_some() {
        // All three are potentially compatible; interface priority rule is
        // applied in resolve_token_program.
        return &[
            ProgramCategory::TokenInterface,
            ProgramCategory::Token,
            ProgramCategory::Token2022,
        ];
    }

    // Unknown wrapper (shouldn't happen for token ops) — allow all.
    &[
        ProgramCategory::TokenInterface,
        ProgramCategory::Token,
        ProgramCategory::Token2022,
    ]
}

/// Resolve a single token program for an op-bearing field.
/// Applies account-aware filtering and interface priority rule.
fn resolve_token_program(
    candidates: &[ProgramCandidate],
    effective_ty: &Type,
    span: &syn::Field,
) -> syn::Result<syn::Ident> {
    let compatible = compatible_token_categories(effective_ty);
    let filtered: Vec<&ProgramCandidate> = candidates
        .iter()
        .filter(|c| compatible.contains(&c.category))
        .collect();

    // Interface priority for InterfaceAccount fields.
    let is_interface_account = extract_generic_inner_type(effective_ty, "InterfaceAccount").is_some();
    if is_interface_account {
        let interfaces: Vec<&ProgramCandidate> = filtered
            .iter()
            .filter(|c| c.category == ProgramCategory::TokenInterface)
            .copied()
            .collect();
        if interfaces.len() == 1 {
            return Ok(interfaces[0].ident.clone());
        }
        if interfaces.len() > 1 {
            return Err(syn::Error::new_spanned(
                span,
                "multiple `Interface<TokenInterface>` fields found — specify `token_program = ...` explicitly",
            ));
        }
        // No interface → fall through to concrete programs.
    }

    match filtered.len() {
        0 => Err(syn::Error::new_spanned(
            span,
            "no compatible token program field found. Add a `Program<TokenProgram>`, \
             `Program<Token2022Program>`, or `Interface<TokenInterface>` field, or specify \
             `token_program = ...` explicitly. Program fields inside composite accounts \
             are not considered",
        )),
        1 => Ok(filtered[0].ident.clone()),
        _ => {
            let names: Vec<String> = filtered.iter().map(|c| c.ident.to_string()).collect();
            Err(syn::Error::new_spanned(
                span,
                format!(
                    "ambiguous token program — found {}. Specify `token_program = ...` explicitly",
                    names.join(", "),
                ),
            ))
        }
    }
}

/// Resolve a single program for system or ATA category.
fn resolve_simple_program(
    candidates: &[ProgramCandidate],
    category: ProgramCategory,
    arg_name: &str,
    type_name: &str,
    span: &syn::Field,
) -> syn::Result<syn::Ident> {
    let filtered: Vec<&ProgramCandidate> = candidates
        .iter()
        .filter(|c| c.category == category)
        .collect();
    match filtered.len() {
        0 => Err(syn::Error::new_spanned(
            span,
            format!(
                "no `{type_name}` field found. Add one to the accounts struct, or specify \
                 `{arg_name} = ...` explicitly. Program fields inside composite accounts \
                 are not considered",
            ),
        )),
        1 => Ok(filtered[0].ident.clone()),
        _ => Err(syn::Error::new_spanned(
            span,
            format!(
                "multiple `{type_name}` fields found — specify `{arg_name} = ...` explicitly",
            ),
        )),
    }
}

/// Build a synthetic GroupArg from a resolved field ident.
fn synthetic_arg(key: &str, field_ident: &syn::Ident) -> GroupArg {
    GroupArg {
        key: format_ident!("{}", key),
        value: syn::parse_quote! { #field_ident },
    }
}

/// Check if a group already has an arg with the given key.
fn has_arg(group: &super::GroupDirective, key: &str) -> bool {
    group.args.iter().any(|a| a.key == key)
}

/// Scan struct fields for program types and inject missing program args
/// into op groups.
///
/// This is the first cross-field resolution mechanism in the derive.
/// Existing resolution (payer, address, realloc) is per-field. Program
/// inference is different: a `Program<TokenProgram>` field is consumed
/// by other fields' op groups.
fn resolve_program_args(semantics: &mut [FieldSemantics]) -> syn::Result<()> {
    // Step 1: Scan all fields for program candidates.
    let candidates: Vec<ProgramCandidate> = semantics
        .iter()
        .filter_map(|sem| {
            let cat = classify_program(
                &sem.core.effective_ty,
                sem.core.optional,
                sem.core.dup,
                sem.core.kind,
            )?;
            Some(ProgramCandidate {
                ident: sem.core.ident.clone(),
                category: cat,
            })
        })
        .collect();

    // Step 2: For each field with groups, inject missing program args.
    for sem in semantics.iter_mut() {
        for group in &mut sem.groups {
            let kind = match GroupKind::from_path(&group.path) {
                Ok(k) => k,
                Err(_) => continue,
            };

            match kind {
                GroupKind::Token | GroupKind::Mint => {
                    if !has_arg(group, "token_program") {
                        let resolved = resolve_token_program(
                            &candidates,
                            &sem.core.effective_ty,
                            &sem.core.field,
                        )?;
                        group.args.push(synthetic_arg("token_program", &resolved));
                    }
                }
                GroupKind::AssociatedToken => {
                    if !has_arg(group, "token_program") {
                        let resolved = resolve_token_program(
                            &candidates,
                            &sem.core.effective_ty,
                            &sem.core.field,
                        )?;
                        group.args.push(synthetic_arg("token_program", &resolved));
                    }
                    // system_program and ata_program only needed for init.
                    if sem.init.is_some() {
                        if !has_arg(group, "system_program") {
                            let resolved = resolve_simple_program(
                                &candidates,
                                ProgramCategory::System,
                                "system_program",
                                concat!("Program<", "System", "Program", ">"),
                                &sem.core.field,
                            )?;
                            group.args.push(synthetic_arg("system_program", &resolved));
                        }
                        if !has_arg(group, "ata_program") {
                            let resolved = resolve_simple_program(
                                &candidates,
                                ProgramCategory::Ata,
                                "ata_program",
                                concat!("Program<", "Associated", "Token", "Program", ">"),
                                &sem.core.field,
                            )?;
                            group.args.push(synthetic_arg("ata_program", &resolved));
                        }
                    }
                }
                GroupKind::Close => {
                    // Only inject token_program for token close (has authority).
                    if has_arg(group, "authority") && !has_arg(group, "token_program") {
                        let resolved = resolve_token_program(
                            &candidates,
                            &sem.core.effective_ty,
                            &sem.core.field,
                        )?;
                        group.args.push(synthetic_arg("token_program", &resolved));
                    }
                }
                GroupKind::Sweep => {
                    if !has_arg(group, "token_program") {
                        let resolved = resolve_token_program(
                            &candidates,
                            &sem.core.effective_ty,
                            &sem.core.field,
                        )?;
                        group.args.push(synthetic_arg("token_program", &resolved));
                    }
                }
            }
        }
    }

    Ok(())
}

// --- Op classification ---

/// Classify groups into buckets and sort exit actions.
fn classify_groups(mut sem: FieldSemantics) -> syn::Result<FieldSemantics> {
    for group in &sem.groups {
        let op = GroupOp::from_directive(group)?;
        match op.kind.op_kind() {
            OpKind::Check => {
                sem.constraints.push(op.clone());
                if sem.has_init() {
                    sem.init_contributors.push(op);
                }
            }
            OpKind::Exit => {
                sem.exit_actions.push(op);
            }
        }
    }

    // Sort exit_actions: sweep before close.
    sem.exit_actions.sort_by_key(|g| g.kind.exit_order());

    Ok(sem)
}
