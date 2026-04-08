//! Constraint parsing for `#[derive(Accounts)]` struct fields.
//!
//! Classifies account field wrapper types (`Account<T>`, `Signer`, etc.) and
//! extracts constraint directives from `#[account(...)]` attributes.

/// Classification of an account field's wrapper type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldClass {
    Account { inner_type: String },
    TokenAccount,
    Mint,
    Signer,
    Program,
    Sysvar,
    SystemAccount,
    Unchecked,
}

impl FieldClass {
    /// Returns true if the type itself provides validation (no extra constraint
    /// needed to prove identity).
    pub fn is_self_constrained(&self) -> bool {
        matches!(self, Self::Signer | Self::Program | Self::Sysvar)
    }
}

/// Parsed constraint directives from `#[account(...)]` attributes on a field.
#[derive(Debug, Default, Clone)]
pub struct FieldConstraints {
    pub is_mut: bool,
    pub is_init: bool,
    pub has_ones: Vec<String>,
    pub token_mint: Option<String>,
    pub token_authority: Option<String>,
    pub associated_token_mint: Option<String>,
    pub associated_token_authority: Option<String>,
    pub seeds_account_refs: Vec<String>,
    pub has_address: bool,
    pub has_constraint: bool,
    pub has_close: bool,
    pub payer: Option<String>,
    pub suppressions: Vec<String>,
}

/// Classify a `syn::Type` into a `FieldClass` and optionally extract the inner
/// type name (e.g. `Account<Vault<'info>>` → `(Account { inner_type: "Vault" },
/// Some("Vault"))`).
pub fn classify_field_type(ty: &syn::Type) -> (FieldClass, Option<String>) {
    let ty = strip_reference(ty);

    let type_path = match ty {
        syn::Type::Path(tp) => tp,
        _ => return (FieldClass::Unchecked, None),
    };

    let last_seg = match type_path.path.segments.last() {
        Some(seg) => seg,
        None => return (FieldClass::Unchecked, None),
    };

    let ident = last_seg.ident.to_string();
    let inner_name = extract_inner_type_name(last_seg);

    match ident.as_str() {
        "Account" => {
            let inner = inner_name.clone().unwrap_or_else(|| "Unknown".to_string());
            (FieldClass::Account { inner_type: inner.clone() }, Some(inner))
        }
        "InterfaceAccount" => {
            // Distinguish TokenAccount vs Mint by inner type name
            let inner = inner_name.clone().unwrap_or_default();
            if inner == "Mint" {
                (FieldClass::Mint, inner_name)
            } else {
                // Default: treat as TokenAccount for InterfaceAccount<TokenAccount>
                (FieldClass::TokenAccount, inner_name)
            }
        }
        "TokenAccount" => (FieldClass::TokenAccount, inner_name),
        "Mint" => (FieldClass::Mint, inner_name),
        "Signer" => (FieldClass::Signer, None),
        "Program" => (FieldClass::Program, inner_name),
        "Sysvar" => (FieldClass::Sysvar, inner_name),
        "SystemAccount" => (FieldClass::SystemAccount, None),
        "UncheckedAccount" | "AccountInfo" => (FieldClass::Unchecked, None),
        _ => {
            // Unknown wrapper — default to Unchecked
            (FieldClass::Unchecked, None)
        }
    }
}

/// Parse all `#[account(...)]` and `#[allow(quasar::*)]` attributes on a field.
pub fn parse_field_constraints(attrs: &[syn::Attribute]) -> FieldConstraints {
    let mut c = FieldConstraints::default();

    for attr in attrs {
        // Parse #[allow(quasar::...)] suppressions
        if attr.path().is_ident("allow") {
            if let Ok(list) = attr.meta.require_list() {
                let tokens_str = list.tokens.to_string();
                for part in tokens_str.split(',') {
                    // Normalize spaces around `::` that syn inserts
                    let normalized: String = part.split_whitespace().collect::<Vec<_>>().join("");
                    if normalized.starts_with("quasar::") {
                        c.suppressions.push(normalized);
                    }
                }
            }
            continue;
        }

        if !attr.path().is_ident("account") {
            continue;
        }

        let tokens_str = match attr.meta.require_list() {
            Ok(list) => list.tokens.to_string(),
            Err(_) => continue,
        };

        let directives = split_directives(&tokens_str);

        for directive in &directives {
            let d = directive.trim();

            if d == "mut" {
                c.is_mut = true;
            } else if d == "init" || d == "init_if_needed" {
                c.is_init = true;
                c.is_mut = true;
            } else if d.starts_with("has_one") {
                if let Some(ident) = extract_eq_ident(d) {
                    c.has_ones.push(ident);
                }
            } else if d.starts_with("token :: mint") || d.starts_with("token::mint") {
                c.token_mint = extract_eq_ident(d);
            } else if d.starts_with("token :: authority") || d.starts_with("token::authority") {
                c.token_authority = extract_eq_ident(d);
            } else if d.starts_with("associated_token :: mint")
                || d.starts_with("associated_token::mint")
            {
                c.associated_token_mint = extract_eq_ident(d);
            } else if d.starts_with("associated_token :: authority")
                || d.starts_with("associated_token::authority")
            {
                c.associated_token_authority = extract_eq_ident(d);
            } else if d.starts_with("address") {
                c.has_address = true;
            } else if d.starts_with("constraint") {
                c.has_constraint = true;
            } else if d.starts_with("close") {
                c.has_close = true;
            } else if d.starts_with("payer") {
                c.payer = extract_eq_ident(d);
            } else if d.starts_with("seeds") {
                let refs = extract_seed_account_refs(d);
                c.seeds_account_refs.extend(refs);
            }
        }
    }

    c
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Strip outer `&` / `&'a` / `&'a mut` from a type.
fn strip_reference(ty: &syn::Type) -> &syn::Type {
    match ty {
        syn::Type::Reference(r) => strip_reference(&r.elem),
        other => other,
    }
}

/// Extract the inner type name from angle brackets, skipping lifetime params.
/// e.g. `Account<'info, Vault>` → Some("Vault"), `Account<Vault<'info>>` →
/// Some("Vault")
fn extract_inner_type_name(seg: &syn::PathSegment) -> Option<String> {
    let args = match &seg.arguments {
        syn::PathArguments::AngleBracketed(ab) => ab,
        _ => return None,
    };

    // Find the first Type argument (skip lifetimes)
    for arg in &args.args {
        if let syn::GenericArgument::Type(inner_ty) = arg {
            // Get the base name of this type (strips its own generics)
            return type_base_name(inner_ty);
        }
    }

    None
}

/// Get the last path segment name from a type.
fn type_base_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_base_name(&r.elem),
        _ => None,
    }
}

/// Split a directive string by commas, respecting nested brackets, parens, and
/// string literals.
fn split_directives(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;

    for c in s.chars() {
        match c {
            '"' if !in_string => {
                in_string = true;
                current.push(c);
            }
            '"' if in_string => {
                in_string = false;
                current.push(c);
            }
            '[' | '(' if !in_string => {
                depth += 1;
                current.push(c);
            }
            ']' | ')' if !in_string => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 && !in_string => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }

    parts
}

/// Extract the identifier after `=` in a directive like `has_one = wallet`.
/// Also strips `@ ErrorCode::...` suffixes.
fn extract_eq_ident(s: &str) -> Option<String> {
    let eq_pos = s.find('=')?;
    let after_eq = s[eq_pos + 1..].trim();

    // Strip @ error suffix: `wallet @ MyError::BadWallet` → `wallet`
    let value = if let Some(at_pos) = after_eq.find('@') {
        after_eq[..at_pos].trim()
    } else {
        after_eq
    };

    // Take just the identifier (alphanumeric + underscore)
    let ident: String = value
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

/// Extract account references from a `seeds = [...]` directive.
/// Looks for patterns like `ident.key()` or `ident.address()`.
fn extract_seed_account_refs(seeds_directive: &str) -> Vec<String> {
    // Find the bracket contents: seeds = [...]
    let bracket_start = match seeds_directive.find('[') {
        Some(idx) => idx,
        None => return vec![],
    };

    let mut depth = 0;
    let mut bracket_end = None;
    for (i, c) in seeds_directive[bracket_start..].chars().enumerate() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    bracket_end = Some(bracket_start + i);
                    break;
                }
            }
            _ => {}
        }
    }

    let bracket_end = match bracket_end {
        Some(idx) => idx,
        None => return vec![],
    };

    let inner = &seeds_directive[bracket_start + 1..bracket_end];

    // Split seed expressions by comma (respecting nesting)
    let seed_exprs = split_directives(inner);

    let mut refs = Vec::new();
    for expr in &seed_exprs {
        let expr = expr.trim();

        // Skip byte string literals — check for `b"` prefix specifically so
        // names like `borrower` are not excluded.
        if expr.starts_with("b\"") || expr.starts_with("b \"") {
            continue;
        }

        // Normalize whitespace so syn-tokenized `wallet . key ()` matches
        // the expected `wallet.key()` pattern.
        let normalized: String = expr.split_whitespace().collect::<Vec<_>>().join("");

        // Look for `ident.key()` or `ident.address()` patterns
        if let Some(dot_pos) = normalized.find('.') {
            let before_dot = &normalized[..dot_pos];
            let after_dot = &normalized[dot_pos + 1..];
            if (after_dot.starts_with("key()") || after_dot.starts_with("address()"))
                && before_dot
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_')
                && !before_dot.is_empty()
            {
                refs.push(before_dot.to_string());
            }
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_directives_basic() {
        let result = split_directives("mut, has_one = wallet, has_one = intent");
        assert_eq!(result, vec!["mut", "has_one = wallet", "has_one = intent"]);
    }

    #[test]
    fn split_directives_with_seeds() {
        let result = split_directives(
            r#"seeds = [b"escrow", maker.key()], bump, has_one = maker"#,
        );
        assert_eq!(result.len(), 3);
        assert!(result[0].starts_with("seeds"));
        assert_eq!(result[1], "bump");
        assert_eq!(result[2], "has_one = maker");
    }

    #[test]
    fn extract_eq_ident_basic() {
        assert_eq!(extract_eq_ident("has_one = wallet"), Some("wallet".to_string()));
    }

    #[test]
    fn extract_eq_ident_with_error() {
        assert_eq!(
            extract_eq_ident("has_one = wallet @ MyError::BadWallet"),
            Some("wallet".to_string())
        );
    }

    #[test]
    fn extract_seed_refs() {
        let refs = extract_seed_account_refs(
            r#"seeds = [b"escrow", maker.key(), borrower.address()]"#,
        );
        assert_eq!(refs, vec!["maker", "borrower"]);
    }

    #[test]
    fn extract_seed_refs_does_not_exclude_borrower() {
        // Regression: must check `b"` prefix, not just `b`, so `borrower` isn't excluded
        let refs = extract_seed_account_refs(
            r#"seeds = [b"vault", borrower.key()]"#,
        );
        assert_eq!(refs, vec!["borrower"]);
    }

    #[test]
    fn parse_seeds_from_syn() {
        let code = r#"
            pub struct Foo {
                #[account(seeds = [b"proposal", wallet.key()], bump)]
                pub x: u32,
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            if let syn::Fields::Named(fields) = &s.fields {
                let field = &fields.named[0];
                let c = parse_field_constraints(&field.attrs);
                assert!(
                    c.seeds_account_refs.contains(&"wallet".to_string()),
                    "expected wallet in seeds_account_refs, got: {:?}",
                    c.seeds_account_refs
                );
            }
        }
    }

    #[test]
    fn parse_allow_suppression() {
        let code = r#"
            pub struct Foo {
                #[allow(quasar::unconstrained)]
                pub x: u32,
            }
        "#;
        let file: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            if let syn::Fields::Named(fields) = &s.fields {
                let field = &fields.named[0];
                let c = parse_field_constraints(&field.attrs);
                assert!(
                    c.suppressions.contains(&"quasar::unconstrained".to_string()),
                    "expected quasar::unconstrained suppression, got: {:?}",
                    c.suppressions
                );
            }
        }
    }

    #[test]
    fn classify_signer() {
        let ty: syn::Type = syn::parse_str("Signer").unwrap();
        let (class, inner) = classify_field_type(&ty);
        assert_eq!(class, FieldClass::Signer);
        assert!(inner.is_none());
        assert!(class.is_self_constrained());
    }

    #[test]
    fn classify_account_with_inner() {
        let ty: syn::Type = syn::parse_str("Account<Vault>").unwrap();
        let (class, inner) = classify_field_type(&ty);
        assert_eq!(
            class,
            FieldClass::Account {
                inner_type: "Vault".to_string()
            }
        );
        assert_eq!(inner, Some("Vault".to_string()));
    }

    #[test]
    fn classify_unchecked_account() {
        let ty: syn::Type = syn::parse_str("UncheckedAccount").unwrap();
        let (class, _) = classify_field_type(&ty);
        assert_eq!(class, FieldClass::Unchecked);
    }
}
