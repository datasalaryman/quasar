use syn::{
    parse::{Parse, ParseStream},
    Expr, ExprArray, Ident, Token,
};

pub(super) enum AccountDirective {
    Mut,
    HasOne(Ident, Option<Expr>),
    Constraint(Expr, Option<Expr>),
    Seeds(Vec<Expr>),
    Bump(Option<Expr>),
    Address(Expr, Option<Expr>),
}

impl Parse for AccountDirective {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(Token![mut]) {
            let _: Token![mut] = input.parse()?;
            return Ok(Self::Mut);
        }
        let key: Ident = input.parse()?;
        match key.to_string().as_str() {
            "has_one" => {
                let _: Token![=] = input.parse()?;
                let ident: Ident = input.parse()?;
                let error = if input.peek(Token![@]) {
                    input.parse::<Token![@]>()?;
                    Some(input.parse::<Expr>()?)
                } else {
                    None
                };
                Ok(Self::HasOne(ident, error))
            }
            "constraint" => {
                let _: Token![=] = input.parse()?;
                let expr: Expr = input.parse()?;
                let error = if input.peek(Token![@]) {
                    input.parse::<Token![@]>()?;
                    Some(input.parse::<Expr>()?)
                } else {
                    None
                };
                Ok(Self::Constraint(expr, error))
            }
            "address" => {
                let _: Token![=] = input.parse()?;
                let expr: Expr = input.parse()?;
                let error = if input.peek(Token![@]) {
                    input.parse::<Token![@]>()?;
                    Some(input.parse::<Expr>()?)
                } else {
                    None
                };
                Ok(Self::Address(expr, error))
            }
            "seeds" => {
                let _: Token![=] = input.parse()?;
                let arr: ExprArray = input.parse()?;
                Ok(Self::Seeds(arr.elems.into_iter().collect()))
            }
            "bump" => {
                if input.peek(Token![=]) {
                    let _: Token![=] = input.parse()?;
                    Ok(Self::Bump(Some(input.parse()?)))
                } else {
                    Ok(Self::Bump(None))
                }
            }
            _ => Err(syn::Error::new(
                key.span(),
                format!("unknown account attribute: `{}`", key),
            )),
        }
    }
}

pub(super) struct AccountFieldAttrs {
    pub is_mut: bool,
    pub has_ones: Vec<(Ident, Option<Expr>)>,
    pub constraints: Vec<(Expr, Option<Expr>)>,
    pub seeds: Option<Vec<Expr>>,
    pub bump: Option<Option<Expr>>,
    pub address: Option<(Expr, Option<Expr>)>,
}

impl Parse for AccountFieldAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let directives = input.parse_terminated(AccountDirective::parse, Token![,])?;
        let mut is_mut = false;
        let mut has_ones = Vec::new();
        let mut constraints = Vec::new();
        let mut seeds = None;
        let mut bump = None;
        let mut address = None;
        for d in directives {
            match d {
                AccountDirective::Mut => is_mut = true,
                AccountDirective::HasOne(ident, err) => has_ones.push((ident, err)),
                AccountDirective::Constraint(expr, err) => constraints.push((expr, err)),
                AccountDirective::Seeds(s) => seeds = Some(s),
                AccountDirective::Bump(b) => bump = Some(b),
                AccountDirective::Address(expr, err) => address = Some((expr, err)),
            }
        }
        Ok(Self {
            is_mut,
            has_ones,
            constraints,
            seeds,
            bump,
            address,
        })
    }
}

pub(super) fn parse_field_attrs(field: &syn::Field) -> syn::Result<AccountFieldAttrs> {
    for attr in &field.attrs {
        if attr.path().is_ident("account") {
            return attr.parse_args::<AccountFieldAttrs>();
        }
    }
    Ok(AccountFieldAttrs {
        is_mut: false,
        has_ones: vec![],
        constraints: vec![],
        seeds: None,
        bump: None,
        address: None,
    })
}
