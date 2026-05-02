//! Structural validation rules — no domain knowledge.

use super::{FieldSemantics, GroupArg, GroupKind};

pub(super) fn validate_semantics(semantics: &[FieldSemantics]) -> syn::Result<()> {
    for sem in semantics {
        validate_field(sem)?;
    }
    Ok(())
}

fn validate_field(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;

    // --- Migration exclusivity rules ---
    if sem.is_migration {
        if !sem.core.is_mut {
            return Err(syn::Error::new_spanned(
                span,
                "`Migration<From, To>` requires `mut`",
            ));
        }
        if sem.payer.is_none() {
            return Err(syn::Error::new_spanned(
                span,
                "`Migration<From, To>` requires `payer = ...`",
            ));
        }
        if sem.core.optional {
            return Err(syn::Error::new_spanned(
                span,
                "`Option<Migration<...>>` is not supported — migration fields cannot be optional",
            ));
        }
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "`init` cannot be used with `Migration<From, To>`",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc` cannot be used with `Migration<From, To>`",
            ));
        }
        for group in &sem.groups {
            let kind = GroupKind::from_path(&group.path)?;
            if matches!(kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!(
                        "`{}` cannot be used with `Migration<From, To>` — migrating and closing \
                         the same account is ordering-sensitive",
                        kind.name()
                    ),
                ));
            }
        }
    }

    // init requires mut
    if sem.has_init() && !sem.core.is_mut {
        return Err(syn::Error::new_spanned(span, "`init(...)` requires `mut`"));
    }

    // init + realloc mutual exclusion
    if sem.has_init() && sem.realloc.is_some() {
        return Err(syn::Error::new_spanned(
            span,
            "`realloc = ...` cannot be used with `init`",
        ));
    }

    // dup + mutation ops blocked (init, realloc, exit ops)
    if sem.core.dup {
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "`dup` cannot be used with `init` — mutation on aliased accounts is unsound",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`dup` cannot be used with `realloc` — mutation on aliased accounts is unsound",
            ));
        }
        for group in &sem.groups {
            let kind = GroupKind::from_path(&group.path)?;
            if matches!(kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!(
                        "`dup` cannot be used with `{}` — mutation on aliased accounts is unsound",
                        kind.name()
                    ),
                ));
            }
        }
    }

    // Exit ops require mut
    if !sem.core.is_mut {
        for group in &sem.groups {
            let kind = GroupKind::from_path(&group.path)?;
            if matches!(kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("`{}(...)` requires `mut`", kind.name()),
                ));
            }
        }
    }

    // sweep-before-close hard error: close must come AFTER sweep.
    validate_exit_ordering(sem)?;
    validate_close_groups(sem)?;

    // dup requires /// CHECK: doc comment
    if sem.core.dup {
        let has_doc = sem
            .core
            .field
            .attrs
            .iter()
            .any(|a| a.path().is_ident("doc"));
        if !has_doc {
            return Err(syn::Error::new_spanned(
                span,
                "#[account(dup)] requires a /// CHECK: <reason> doc comment",
            ));
        }
    }

    // Optional accounts cannot have init or op groups
    if sem.core.optional {
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "init(...) cannot be used on Option<T> fields",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc = ...` cannot be used on Option<T> fields",
            ));
        }
        if !sem.groups.is_empty() {
            return Err(syn::Error::new_spanned(
                span,
                "op groups cannot be used on Option<T> fields (only has_one/address/constraints)",
            ));
        }
    }

    if sem.realloc.is_some() {
        if !sem.core.is_mut {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc = ...` requires `mut`",
            ));
        }
        if sem.payer.is_none() {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc = ...` requires `payer = ...` on the same field",
            ));
        }
    }

    // Zero or one init contributor per field. Multiple SPL init groups
    // (e.g., token(...) + associated_token(...)) is a compile error.
    if sem.has_init() {
        let init_contributor_count = sem
            .groups
            .iter()
            .filter(|group| {
                matches!(
                    GroupKind::from_path(&group.path),
                    Ok(GroupKind::Token | GroupKind::Mint | GroupKind::AssociatedToken)
                )
            })
            .count();
        if init_contributor_count > 1 {
            return Err(syn::Error::new_spanned(
                span,
                "only one init contributor group is allowed per field (e.g., `token(...)` or \
                 `associated_token(...)`, not both)",
            ));
        }
    }

    // init(idempotent) requires a validation group or address constraint
    if let Some(init) = &sem.init {
        if init.idempotent {
            let has_validation = !sem.groups.is_empty();
            let has_address = sem.address.is_some();
            if !has_validation && !has_address {
                return Err(syn::Error::new_spanned(
                    span,
                    "`init(idempotent)` requires a validation group (e.g., token(...)) or address \
                     constraint",
                ));
            }
        }
    }

    // Validate required args per group kind.
    validate_required_group_args(sem)?;

    Ok(())
}

/// Validate close args and reject ambiguous close forms.
fn validate_close_groups(sem: &FieldSemantics) -> syn::Result<()> {
    let has_sweep = sem
        .groups
        .iter()
        .any(|group| matches!(GroupKind::from_path(&group.path), Ok(GroupKind::Sweep)));

    for group in &sem.groups {
        if !matches!(GroupKind::from_path(&group.path)?, GroupKind::Close) {
            continue;
        }

        for arg in &group.args {
            if !matches!(
                arg.key.to_string().as_str(),
                "dest" | "authority" | "token_program"
            ) {
                return Err(syn::Error::new_spanned(
                    &arg.key,
                    format!(
                        "unknown `close(...)` arg `{}`. Valid forms: `close(dest = ...)` for \
                         program accounts, or `close(dest = ..., authority = ..., token_program = \
                         ...)` for token accounts",
                        arg.key
                    ),
                ));
            }
        }

        if !has_arg(&group.args, "dest") {
            return Err(syn::Error::new_spanned(
                &group.path,
                "`close(...)` requires `dest = ...`",
            ));
        }

        let has_authority = has_arg(&group.args, "authority");
        let has_token_program = has_arg(&group.args, "token_program");
        if has_authority != has_token_program {
            return Err(syn::Error::new_spanned(
                &group.path,
                "`close(...)` must include both `authority = ...` and `token_program = ...` for \
                 token accounts, or neither for program accounts",
            ));
        }

        if has_sweep && !has_authority {
            return Err(syn::Error::new_spanned(
                &group.path,
                "`sweep(...)` can only be paired with token close. Use `close(dest = ..., \
                 authority = ..., token_program = ...)`",
            ));
        }
    }

    Ok(())
}

/// Validate exit action ordering: sweep must come before close.
fn validate_exit_ordering(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;
    let mut seen_close = false;

    for group in &sem.groups {
        let kind = GroupKind::from_path(&group.path)?;
        match kind {
            GroupKind::Close => {
                seen_close = true;
            }
            GroupKind::Sweep => {
                if seen_close {
                    return Err(syn::Error::new_spanned(
                        span,
                        "`sweep(...)` must appear before `close(...)` — wrong ordering is always \
                         a bug",
                    ));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Validate that each group has its required args.
/// Runs after program inference, so inferred args are already present.
///
/// `token_program` is NOT required for `token(...)` and `mint(...)` —
/// for concrete account types (Account<Token>), AccountLoad already
/// validates the owner. token_program is only needed for InterfaceAccount,
/// where inference will have injected it.
fn validate_required_group_args(sem: &FieldSemantics) -> syn::Result<()> {
    for group in &sem.groups {
        let kind = GroupKind::from_path(&group.path)?;
        let required: &[&str] = match kind {
            GroupKind::Token => &["mint", "authority"],
            GroupKind::Mint => &["authority"],
            GroupKind::AssociatedToken => {
                if sem.has_init() {
                    &["mint", "authority", "token_program", "system_program", "ata_program"]
                } else {
                    // token_program is optional for validation-only concrete types.
                    // Inference will have injected it if a program field exists.
                    &["mint", "authority"]
                }
            }
            GroupKind::Close => continue, // validated by validate_close_groups
            GroupKind::Sweep => &["receiver", "mint", "authority", "token_program"],
        };
        for &key in required {
            if !has_arg(&group.args, key) {
                return Err(syn::Error::new_spanned(
                    &group.path,
                    format!("`{}(...)` requires `{key} = ...`", kind.name()),
                ));
            }
        }
    }
    Ok(())
}

fn has_arg(args: &[GroupArg], key: &str) -> bool {
    args.iter().any(|arg| arg.key == key)
}
