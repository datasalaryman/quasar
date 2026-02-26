mod attrs;
mod client;
mod fields;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

use crate::helpers::{is_composite_type, strip_generics};

pub(crate) fn derive_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let bumps_name = format_ident!("{}Bumps", name);

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new_spanned(
                    name,
                    "Accounts can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(name, "Accounts can only be derived for structs")
                .to_compile_error()
                .into();
        }
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    let field_name_strings: Vec<String> = fields
        .iter()
        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
        .collect();

    let mut pf = match fields::process_fields(fields, &field_name_strings) {
        Ok(pf) => pf,
        Err(ts) => return ts,
    };

    // --- Composite type handling ---

    let mut has_composites = false;
    let mut composite_types: Vec<Option<proc_macro2::TokenStream>> = Vec::new();
    for field in fields.iter() {
        if is_composite_type(&field.ty) {
            has_composites = true;
            composite_types.push(Some(strip_generics(&field.ty)));
        } else {
            composite_types.push(None);
        }
    }

    let count_expr: proc_macro2::TokenStream = if has_composites {
        let addends: Vec<proc_macro2::TokenStream> = composite_types
            .iter()
            .map(|ct| match ct {
                Some(ty) => quote! { <#ty as AccountCount>::COUNT },
                None => quote! { 1usize },
            })
            .collect();
        quote! { #(#addends)+* }
    } else {
        let field_count = field_names.len();
        quote! { #field_count }
    };

    let mut parse_steps: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut buf_offset = quote! { 0usize };
    for ct in &composite_types {
        if let Some(inner_ty) = ct {
            let cur_offset = buf_offset.clone();
            parse_steps.push(quote! {
                {
                    let mut __inner_buf = core::mem::MaybeUninit::<
                        [quasar_core::__internal::AccountView; <#inner_ty as AccountCount>::COUNT]
                    >::uninit();
                    input = <#inner_ty>::parse_accounts(input, &mut __inner_buf);
                    let __inner = unsafe { __inner_buf.assume_init() };
                    let mut __j = 0usize;
                    while __j < <#inner_ty as AccountCount>::COUNT {
                        unsafe { core::ptr::write(base.add(#cur_offset + __j), *__inner.as_ptr().add(__j)); }
                        __j += 1;
                    }
                }
            });
            buf_offset = quote! { #buf_offset + <#inner_ty as AccountCount>::COUNT };
        } else {
            let cur_offset = buf_offset.clone();
            parse_steps.push(quote! {
                {
                    let raw = input as *mut quasar_core::__internal::RuntimeAccount;
                    if unsafe { (*raw).borrow_state } == quasar_core::__internal::NOT_BORROWED {
                        unsafe {
                            core::ptr::write(base.add(#cur_offset), quasar_core::__internal::AccountView::new_unchecked(raw));
                            input = input.add(__ACCOUNT_HEADER + (*raw).data_len as usize);
                            let addr = input as usize;
                            input = ((addr + 7) & !7) as *mut u8;
                        }
                    } else {
                        unsafe {
                            let idx = (*raw).borrow_state as usize;
                            core::ptr::write(base.add(#cur_offset), core::ptr::read(base.add(idx)));
                            input = input.add(core::mem::size_of::<u64>());
                        }
                    }
                }
            });
            buf_offset = quote! { #buf_offset + 1usize };
        }
    }

    // --- Composite field_lets (pre-compute before bumps so pushes take effect) ---

    let has_pda_fields = !pf.bump_struct_fields.is_empty();

    let mut field_lets: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut non_composite_constructs: Vec<proc_macro2::TokenStream> = Vec::new();
    if has_composites {
        let mut idx_offset = quote! { 0usize };
        for (fi, field) in fields.iter().enumerate() {
            let field_name = field.ident.as_ref().unwrap();
            if composite_types[fi].is_some() {
                let inner_ty = composite_types[fi].as_ref().unwrap();
                let bumps_var = format_ident!("__composite_bumps_{}", field_name);
                let cur_offset = idx_offset.clone();
                field_lets.push(quote! {
                    let (#field_name, #bumps_var) = <#inner_ty as ParseAccounts>::parse(
                        &accounts[#cur_offset..#cur_offset + <#inner_ty as AccountCount>::COUNT]
                    )?;
                });
                pf.bump_struct_fields
                    .push(quote! { pub #field_name: <#inner_ty as ParseAccounts>::Bumps });
                pf.bump_struct_inits
                    .push(quote! { #field_name: #bumps_var });
                idx_offset = quote! { #idx_offset + <#inner_ty as AccountCount>::COUNT };
            } else {
                let cur_offset = idx_offset.clone();
                field_lets.push(quote! {
                    let #field_name = &accounts[#cur_offset];
                });
                idx_offset = quote! { #idx_offset + 1usize };
            }
        }

        non_composite_constructs = fields
            .iter()
            .enumerate()
            .map(|(fi, field)| {
                let field_name = field.ident.as_ref().unwrap();
                if composite_types[fi].is_some() {
                    quote! { #field_name }
                } else {
                    pf.field_constructs[fi].clone()
                }
            })
            .collect();
    }

    // --- Bumps (after all modifications including composites) ---

    let bump_struct_fields = &pf.bump_struct_fields;
    let bump_struct_inits = &pf.bump_struct_inits;

    let bumps_struct = if has_pda_fields || !bump_struct_fields.is_empty() {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name { #(#bump_struct_fields,)* } }
    } else {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name; }
    };

    let bumps_init = if has_pda_fields || !bump_struct_inits.is_empty() {
        quote! { #bumps_name { #(#bump_struct_inits,)* } }
    } else {
        quote! { #bumps_name }
    };

    // --- Parse body generation (3 code paths) ---

    let has_any_checks = !pf.has_one_checks.is_empty()
        || !pf.constraint_checks.is_empty()
        || !pf.mut_checks.is_empty()
        || !pf.pda_checks.is_empty();

    let seed_addr_captures = &pf.seed_addr_captures;
    let bump_init_vars = &pf.bump_init_vars;
    let mut_checks = &pf.mut_checks;
    let has_one_checks = &pf.has_one_checks;
    let constraint_checks = &pf.constraint_checks;
    let pda_checks = &pf.pda_checks;
    let field_constructs = &pf.field_constructs;

    let parse_body = if has_composites {
        if has_any_checks {
            quote! {
                if accounts.len() < Self::COUNT {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                #(#field_lets)*
                #(#seed_addr_captures)*

                let result = Self {
                    #(#non_composite_constructs,)*
                };

                #(#bump_init_vars)*

                {
                    let Self { #(ref #field_names,)* } = result;
                    #(#mut_checks)*
                    #(#has_one_checks)*
                    #(#constraint_checks)*
                    #(#pda_checks)*
                }

                Ok((result, #bumps_init))
            }
        } else {
            quote! {
                if accounts.len() < Self::COUNT {
                    return Err(ProgramError::NotEnoughAccountKeys);
                }
                #(#field_lets)*

                Ok((Self {
                    #(#non_composite_constructs,)*
                }, #bumps_init))
            }
        }
    } else if has_any_checks {
        quote! {
            let [#(#field_names),*] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };

            #(#seed_addr_captures)*

            let result = Self {
                #(#field_constructs,)*
            };

            #(#bump_init_vars)*

            {
                let Self { #(ref #field_names,)* } = result;
                #(#mut_checks)*
                #(#has_one_checks)*
                #(#constraint_checks)*
                #(#pda_checks)*
            }

            Ok((result, #bumps_init))
        }
    } else {
        quote! {
            let [#(#field_names),*] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };

            Ok((Self {
                #(#field_constructs,)*
            }, #bumps_init))
        }
    };

    // --- Seeds impl ---

    let seeds_methods = &pf.seeds_methods;
    let seeds_impl = if seeds_methods.is_empty() {
        quote! {}
    } else {
        quote! {
            impl #bumps_name {
                #(#seeds_methods)*
            }
        }
    };

    // --- Client macro ---

    let client_macro = client::generate_client_macro(name, fields, &pf.field_attrs);

    // --- Final output ---

    let expanded = quote! {
        #bumps_struct

        impl<'info> ParseAccounts<'info> for #name<'info> {
            type Bumps = #bumps_name;

            #[inline(always)]
            fn parse(accounts: &'info [AccountView]) -> Result<(Self, Self::Bumps), ProgramError> {
                #parse_body
            }
        }

        #seeds_impl

        impl<'info> AccountCount for #name<'info> {
            const COUNT: usize = #count_expr;
        }

        impl<'info> #name<'info> {
            #[inline(always)]
            pub unsafe fn parse_accounts(
                mut input: *mut u8,
                buf: &mut core::mem::MaybeUninit<[quasar_core::__internal::AccountView; #count_expr]>,
            ) -> *mut u8 {
                const __ACCOUNT_HEADER: usize =
                    core::mem::size_of::<quasar_core::__internal::RuntimeAccount>()
                    + quasar_core::__internal::MAX_PERMITTED_DATA_INCREASE
                    + core::mem::size_of::<u64>();

                let base = buf.as_mut_ptr() as *mut quasar_core::__internal::AccountView;

                #(#parse_steps)*

                input
            }
        }

        #client_macro
    };

    TokenStream::from(expanded)
}
