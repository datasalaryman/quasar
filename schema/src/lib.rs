//! Shared schema utilities consumed by multiple Quasar crates.
//!
//! Keep this crate narrow: only case-conversion utilities and address lookups
//! belong here. The canonical IDL type definitions live in `quasar-idl-schema`.

// ---------------------------------------------------------------------------
// Case-conversion utilities (shared across derive, idl, cli, client)
// ---------------------------------------------------------------------------

/// Convert `PascalCase` to `snake_case`. Handles acronyms (e.g.
/// "HTTPServer" → "http_server") by checking adjacent character case.
pub fn pascal_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let mut prev: Option<char> = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_uppercase() && prev.is_some() {
            let prev_lower = prev.is_some_and(|p| p.is_lowercase());
            let next_lower = chars.peek().is_some_and(|n| n.is_lowercase());
            if prev_lower || next_lower {
                result.push('_');
            }
        }
        result.push(c.to_ascii_lowercase());
        prev = Some(c);
    }
    result
}

/// Convert `snake_case` to `PascalCase`.
pub fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Convert `snake_case` to `camelCase`.
pub fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert `camelCase` to `snake_case` (inverse of `to_camel_case`).
///
/// Uses the simple rule of inserting `_` before every uppercase character.
/// Not suitable for acronym-heavy input like "HTTPServer" — use
/// `pascal_to_snake` for that.
pub fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert `PascalCase` or `camelCase` to `SCREAMING_SNAKE_CASE`.
pub fn to_screaming_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_uppercase());
    }
    result
}

/// Capitalize first character of a `camelCase` string to get `PascalCase`.
pub fn camel_to_pascal(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Known addresses
// ---------------------------------------------------------------------------

pub fn known_address_for_type(base: &str, inner: Option<&str>) -> Option<&'static str> {
    match (base, inner) {
        ("SystemProgram", _) | ("Program", Some("System")) => {
            Some("11111111111111111111111111111111")
        }
        ("Program", Some("Token")) => Some("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
        ("Program", Some("Token2022")) => Some("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
        ("Program", Some("AssociatedTokenProgram")) => {
            Some("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        }
        ("Sysvar", Some("Rent")) => Some("SysvarRent111111111111111111111111111111111"),
        ("Sysvar", Some("Clock")) => Some("SysvarC1ock11111111111111111111111111111111"),
        _ => None,
    }
}
