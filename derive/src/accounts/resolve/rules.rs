//! Structural validation rules — no domain knowledge.

use super::FieldSemantics;

pub(super) fn validate_semantics(semantics: &[FieldSemantics]) -> syn::Result<()> {
    for sem in semantics {
        validate_field(sem)?;
    }
    Ok(())
}

fn validate_field(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;

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
            let name = group_last_segment(&group.path);
            if matches!(name.as_str(), "close" | "close_program" | "sweep") {
                return Err(syn::Error::new_spanned(
                    span,
                    format!(
                        "`dup` cannot be used with `{}` — mutation on aliased accounts is unsound",
                        name
                    ),
                ));
            }
        }
    }

    // Exit ops require mut
    if !sem.core.is_mut {
        for group in &sem.groups {
            let name = group_last_segment(&group.path);
            if matches!(name.as_str(), "close" | "close_program" | "sweep") {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("`{}(...)` requires `mut`", name),
                ));
            }
        }
    }

    // sweep-before-close hard error: close/close_program must come AFTER sweep
    validate_exit_ordering(sem)?;

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

    Ok(())
}

/// Validate exit action ordering: sweep must come before close/close_program.
/// Also validate that sweep only pairs with token close, not close_program.
fn validate_exit_ordering(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;
    let mut seen_close = false;
    let mut has_sweep = false;
    let mut has_close_program = false;

    for group in &sem.groups {
        let name = group_last_segment(&group.path);
        match name.as_str() {
            "close" | "close_program" => {
                seen_close = true;
                if name == "close_program" {
                    has_close_program = true;
                }
            }
            "sweep" => {
                has_sweep = true;
                if seen_close {
                    return Err(syn::Error::new_spanned(
                        span,
                        "`sweep(...)` must appear before `close(...)` / `close_program(...)` — \
                         wrong ordering is always a bug",
                    ));
                }
            }
            _ => {}
        }
    }

    // sweep only pairs with token close, not close_program
    if has_sweep && has_close_program {
        return Err(syn::Error::new_spanned(
            span,
            "`sweep(...)` cannot be used with `close_program(...)` — sweep is for token \
             accounts only",
        ));
    }

    Ok(())
}

/// Extract last path segment name from a group directive path.
fn group_last_segment(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}
