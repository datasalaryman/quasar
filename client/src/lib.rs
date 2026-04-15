use {
    proc_macro2::TokenStream,
    quasar_schema::IdlAccountItem,
    quote::{format_ident, quote},
};

pub fn generate_accounts_macro(name: &syn::Ident, descriptors: &[IdlAccountItem]) -> TokenStream {
    let macro_name = format_ident!("__{}_instruction", pascal_to_snake(&name.to_string()));
    let account_fields: Vec<_> = descriptors.iter().map(emit_account_field).collect();
    let account_fields_with_remaining = account_fields.clone();
    let account_metas: Vec<_> = descriptors.iter().map(emit_account_meta).collect();
    let account_metas_with_remaining = account_metas.clone();

    quote! {
        #[cfg(not(any(target_arch = "bpf", target_os = "solana")))]
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_name {
            ($struct_name:ident, [$($disc:expr),*], {$($arg_name:ident : $arg_ty:ty),*}) => {
                pub struct $struct_name {
                    #(#account_fields)*
                    $(pub $arg_name: $arg_ty,)*
                }

                impl From<$struct_name> for quasar_lang::client::Instruction {
                    fn from(ix: $struct_name) -> quasar_lang::client::Instruction {
                        let accounts = ::alloc::vec![
                            #(#account_metas)*
                        ];
                        let data = {
                            let mut _data = ::alloc::vec![$($disc),*];
                            $(
                                _data.extend_from_slice(
                                    &<$arg_ty as quasar_lang::client::SerializeArg>::serialize_arg(&ix.$arg_name)
                                );
                            )*
                            _data
                        };
                        quasar_lang::client::Instruction {
                            program_id: $crate::ID,
                            accounts,
                            data,
                        }
                    }
                }
            };
            ($struct_name:ident, [$($disc:expr),*], {$($arg_name:ident : $arg_ty:ty),*}, remaining) => {
                pub struct $struct_name {
                    #(#account_fields_with_remaining)*
                    $(pub $arg_name: $arg_ty,)*
                    pub remaining_accounts: ::alloc::vec::Vec<quasar_lang::client::AccountMeta>,
                }

                impl From<$struct_name> for quasar_lang::client::Instruction {
                    fn from(ix: $struct_name) -> quasar_lang::client::Instruction {
                        let mut accounts = ::alloc::vec![
                            #(#account_metas_with_remaining)*
                        ];
                        accounts.extend(ix.remaining_accounts);
                        let data = {
                            let mut _data = ::alloc::vec![$($disc),*];
                            $(
                                _data.extend_from_slice(
                                    &<$arg_ty as quasar_lang::client::SerializeArg>::serialize_arg(&ix.$arg_name)
                                );
                            )*
                            _data
                        };
                        quasar_lang::client::Instruction {
                            program_id: $crate::ID,
                            accounts,
                            data,
                        }
                    }
                }
            };
        }
    }
}

fn emit_account_field(descriptor: &IdlAccountItem) -> TokenStream {
    let ident: syn::Ident = syn::parse_str(&descriptor.name).expect("valid account field name");
    quote! { pub #ident: quasar_lang::prelude::Address, }
}

fn emit_account_meta(descriptor: &IdlAccountItem) -> TokenStream {
    let ident: syn::Ident = syn::parse_str(&descriptor.name).expect("valid account field name");
    if descriptor.writable {
        let signer = descriptor.signer;
        quote! {
            quasar_lang::client::AccountMeta::new(ix.#ident, #signer),
        }
    } else {
        let signer = descriptor.signer;
        quote! {
            quasar_lang::client::AccountMeta::new_readonly(ix.#ident, #signer),
        }
    }
}

fn pascal_to_snake(name: &str) -> String {
    let mut snake = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                snake.push('_');
            }
            for lower in ch.to_lowercase() {
                snake.push(lower);
            }
        } else {
            snake.push(ch);
        }
    }
    snake
}
