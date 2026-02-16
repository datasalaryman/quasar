use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Data, DeriveInput, Expr, FnArg, Fields, Ident, ItemFn, LitInt, Pat, Token, Type,
};

// --- Account field attribute parsing ---

enum AccountDirective {
    HasOne(Ident),
    Constraint(Expr),
}

impl Parse for AccountDirective {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        let _: Token![=] = input.parse()?;
        match key.to_string().as_str() {
            "has_one" => Ok(Self::HasOne(input.parse()?)),
            "constraint" => Ok(Self::Constraint(input.parse()?)),
            _ => Err(syn::Error::new(
                key.span(),
                format!("unknown account attribute: `{}`", key),
            )),
        }
    }
}

struct AccountFieldAttrs {
    has_ones: Vec<Ident>,
    constraints: Vec<Expr>,
}

impl Parse for AccountFieldAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let directives =
            input.parse_terminated(AccountDirective::parse, Token![,])?;
        let mut has_ones = Vec::new();
        let mut constraints = Vec::new();
        for d in directives {
            match d {
                AccountDirective::HasOne(ident) => has_ones.push(ident),
                AccountDirective::Constraint(expr) => constraints.push(expr),
            }
        }
        Ok(Self { has_ones, constraints })
    }
}

fn parse_field_attrs(field: &syn::Field) -> AccountFieldAttrs {
    for attr in &field.attrs {
        if attr.path().is_ident("account") {
            return attr
                .parse_args::<AccountFieldAttrs>()
                .expect("failed to parse #[account(...)] attribute");
        }
    }
    AccountFieldAttrs {
        has_ones: vec![],
        constraints: vec![],
    }
}

// --- Derive Accounts ---

#[proc_macro_derive(Accounts, attributes(account))]
pub fn derive_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Accounts can only be derived for structs with named fields"),
        },
        _ => panic!("Accounts can only be derived for structs"),
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    let field_constructs: Vec<proc_macro2::TokenStream> = fields.iter().map(|f| {
        let name = &f.ident;
        match &f.ty {
            Type::Reference(type_ref) => {
                let base_type = strip_generics(&type_ref.elem);
                if type_ref.mutability.is_some() {
                    quote! { #name: #base_type::from_account_view_mut(#name)? }
                } else {
                    quote! { #name: #base_type::from_account_view(#name)? }
                }
            }
            _ => {
                let base_type = strip_generics(&f.ty);
                quote! { #name: #base_type::from_account_view(#name)? }
            }
        }
    }).collect();

    // Collect has_one and constraint directives from field attributes
    let mut has_one_checks: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut constraint_checks: Vec<proc_macro2::TokenStream> = Vec::new();

    for field in fields.iter() {
        let attrs = parse_field_attrs(field);
        let field_name = field.ident.as_ref().unwrap();

        for target in &attrs.has_ones {
            has_one_checks.push(quote! {
                if #field_name.#target != *#target.to_account_view().address() {
                    return Err(QuasarError::HasOneMismatch.into());
                }
            });
        }

        for expr in &attrs.constraints {
            constraint_checks.push(quote! {
                if !(#expr) {
                    return Err(QuasarError::ConstraintViolation.into());
                }
            });
        }
    }

    let has_any_checks = !has_one_checks.is_empty() || !constraint_checks.is_empty();

    let expanded = if has_any_checks {
        quote! {
            impl<'info> TryFrom<&'info [AccountView]> for #name<'info> {
                type Error = ProgramError;

                #[inline(always)]
                fn try_from(accounts: &'info [AccountView]) -> Result<Self, Self::Error> {
                    let [#(#field_names),*] = accounts else {
                        return Err(ProgramError::NotEnoughAccountKeys);
                    };

                    let result = Self {
                        #(#field_constructs,)*
                    };

                    {
                        let Self { #(ref #field_names,)* } = result;
                        #(#has_one_checks)*
                        #(#constraint_checks)*
                    }

                    Ok(result)
                }
            }
        }
    } else {
        quote! {
            impl<'info> TryFrom<&'info [AccountView]> for #name<'info> {
                type Error = ProgramError;

                #[inline(always)]
                fn try_from(accounts: &'info [AccountView]) -> Result<Self, Self::Error> {
                    let [#(#field_names),*] = accounts else {
                        return Err(ProgramError::NotEnoughAccountKeys);
                    };

                    Ok(Self {
                        #(#field_constructs,)*
                    })
                }
            }
        }
    };

    TokenStream::from(expanded)
}

// --- Instruction macro ---

struct InstructionArgs {
    discriminator: LitInt,
}

impl Parse for InstructionArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        if ident != "discriminator" {
            return Err(syn::Error::new(ident.span(), "expected `discriminator`"));
        }
        let _: Token![=] = input.parse()?;
        let discriminator: LitInt = input.parse()?;
        Ok(Self { discriminator })
    }
}

#[proc_macro_attribute]
pub fn instruction(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as InstructionArgs);
    let mut func = parse_macro_input!(item as ItemFn);
    let discriminator = &args.discriminator;

    let first_arg = match func.sig.inputs.first() {
        Some(FnArg::Typed(pt)) => pt.clone(),
        _ => panic!("#[instruction] requires ctx: Ctx<T> as first parameter"),
    };

    let param_name = &first_arg.pat;
    let param_type = &first_arg.ty;

    let remaining: Vec<_> = func.sig.inputs.iter().skip(1).filter_map(|arg| {
        match arg {
            FnArg::Typed(pt) => Some(pt.clone()),
            _ => None,
        }
    }).collect();

    func.sig.inputs = syn::punctuated::Punctuated::new();
    func.sig.inputs.push(syn::parse_quote!(mut context: Context));

    let stmts = std::mem::take(&mut func.block.stmts);
    let mut new_stmts: Vec<syn::Stmt> = vec![
        syn::parse_quote!(
            if context.data.first() != Some(&#discriminator) {
                return Err(ProgramError::InvalidInstructionData);
            }
        ),
        syn::parse_quote!(
            context.data = &context.data[1..];
        ),
        syn::parse_quote!(
            let #param_name: #param_type = Ctx::new(context)?;
        ),
    ];

    if !remaining.is_empty() {
        let field_names: Vec<Ident> = remaining.iter().map(|pt| {
            match &*pt.pat {
                Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                _ => panic!("#[instruction] parameters must be simple identifiers"),
            }
        }).collect();

        let field_types: Vec<&Type> = remaining.iter().map(|pt| &*pt.ty).collect();

        new_stmts.push(syn::parse_quote!(
            #[repr(C)]
            struct InstructionData {
                #(#field_names: #field_types,)*
            }
        ));

        new_stmts.push(syn::parse_quote!(
            if #param_name.data.len() < core::mem::size_of::<InstructionData>() {
                return Err(ProgramError::InvalidInstructionData);
            }
        ));

        new_stmts.push(syn::parse_quote!(
            let __instruction_data = unsafe { &*(#param_name.data.as_ptr() as *const InstructionData) };
        ));

        for name in &field_names {
            new_stmts.push(syn::parse_quote!(
                let #name = __instruction_data.#name;
            ));
        }
    }

    func.block.stmts = new_stmts.into_iter().chain(stmts).collect();

    quote!(#func).into()
}

// --- Account attribute macro ---

#[proc_macro_attribute]
pub fn account(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as InstructionArgs);
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;
    let discriminator = &args.discriminator;

    let field_types: Vec<_> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => fields.named.iter().map(|f| &f.ty).collect(),
            _ => panic!("#[account] can only be used on structs with named fields"),
        },
        _ => panic!("#[account] can only be used on structs"),
    };

    quote! {
        #[repr(C)]
        #[derive(::wincode::SchemaRead, ::wincode::SchemaWrite)]
        #input

        impl Discriminator for #name {
            const DISCRIMINATOR: u8 = #discriminator;
        }

        impl Space for #name {
            const SPACE: usize = 1 #(+ core::mem::size_of::<#field_types>())*;
        }

        impl Owner for #name {
            const OWNER: Address = crate::ID;
        }

        impl AccountCheck for #name {
            #[inline(always)]
            fn check(view: &AccountView) -> Result<(), ProgramError> {
                if unsafe { *view.borrow_unchecked().get_unchecked(0) } != #discriminator {
                    return Err(ProgramError::InvalidAccountData);
                }
                Ok(())
            }
        }

        impl QuasarAccount for #name {
            #[inline(always)]
            fn deserialize(data: &[u8]) -> Result<Self, ProgramError> {
                ::wincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
            }

            #[inline(always)]
            fn serialize(&self, data: &mut [u8]) -> Result<(), ProgramError> {
                ::wincode::serialize_into(data, self).map_err(|_| ProgramError::InvalidAccountData)
            }
        }

        impl #name {
            #[inline(always)]
            pub fn init(self, account: &mut Initialize<Self>, payer: &AccountView, rent: Option<&Rent>) -> Result<(), ProgramError> {
                self.init_signed(account, payer, rent, &[])
            }

            #[inline(always)]
            pub fn init_signed(self, account: &mut Initialize<Self>, payer: &AccountView, rent: Option<&Rent>, signers: &[quasar::cpi::Signer]) -> Result<(), ProgramError> {
                let view = account.to_account_view();

                use quasar::sysvars::Sysvar;
                let lamports = match rent {
                    Some(rent_account) => rent_account.get()?.try_minimum_balance(Self::SPACE)?,
                    None => quasar::sysvars::rent::Rent::get()?.try_minimum_balance(Self::SPACE)?,
                };

                if view.lamports() == 0 {
                    quasar::cpi::system::CreateAccount {
                        from: payer,
                        to: view,
                        lamports,
                        space: Self::SPACE as u64,
                        owner: &Self::OWNER,
                    }.invoke_signed(signers)?;
                } else {
                    let required = lamports.saturating_sub(view.lamports());
                    if required > 0 {
                        quasar::cpi::system::Transfer {
                            from: payer,
                            to: view,
                            lamports: required,
                        }.invoke_signed(signers)?;
                    }
                    quasar::cpi::system::Assign {
                        account: view,
                        owner: &Self::OWNER,
                    }.invoke_signed(signers)?;
                    unsafe { view.resize_unchecked(Self::SPACE) }?;
                }

                let mut data = view.try_borrow_mut()?;
                data[0] = Self::DISCRIMINATOR;
                self.serialize(&mut data[1..])?;
                Ok(())
            }
        }
    }.into()
}

// --- Helpers ---

fn strip_generics(ty: &Type) -> proc_macro2::TokenStream {
    match ty {
        Type::Path(type_path) => {
            let segments: Vec<_> = type_path
                .path
                .segments
                .iter()
                .map(|seg| &seg.ident)
                .collect();
            quote! { #(#segments)::* }
        }
        _ => panic!("Unsupported field type"),
    }
}

// --- Error code macro ---

#[proc_macro_attribute]
pub fn error_code(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => panic!("#[error_code] can only be used on enums"),
    };

    let mut next_discriminant: u32 = 0;
    let match_arms: Vec<_> = variants.iter().map(|v| {
        let ident = &v.ident;
        if let Some((_, expr)) = &v.discriminant {
            if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(lit_int), .. }) = expr {
                next_discriminant = lit_int.base10_parse::<u32>()
                    .expect("#[error_code] discriminant must be a valid u32");
            } else {
                panic!("#[error_code] discriminant must be an integer literal");
            }
        }
        let value = next_discriminant;
        next_discriminant += 1;
        quote! { #value => Ok(#name::#ident) }
    }).collect();

    quote! {
        #[repr(u32)]
        #input

        impl From<#name> for ProgramError {
            #[inline(always)]
            fn from(e: #name) -> Self {
                ProgramError::Custom(e as u32)
            }
        }

        impl TryFrom<u32> for #name {
            type Error = ProgramError;

            #[inline(always)]
            fn try_from(error: u32) -> Result<Self, Self::Error> {
                match error {
                    #(#match_arms,)*
                    _ => Err(ProgramError::InvalidArgument),
                }
            }
        }
    }.into()
}
