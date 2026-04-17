use {
    super::fixed::PodFieldInfo,
    crate::helpers::{map_to_pod_type, pascal_to_snake},
    quote::{format_ident, quote},
};

pub(super) struct ZcSpec {
    pub zc_name: syn::Ident,
    pub zc_mod: syn::Ident,
    pub zc_path: proc_macro2::TokenStream,
    pub fields: Vec<proc_macro2::TokenStream>,
    /// Native-typed fields for the zeropod schema struct (fixed accounts only).
    pub schema_fields: Vec<proc_macro2::TokenStream>,
}

pub(super) fn build_zc_spec(
    name: &syn::Ident,
    field_infos: &[PodFieldInfo<'_>],
    has_dynamic: bool,
) -> ZcSpec {
    let static_fields: Vec<_> = field_infos
        .iter()
        .filter(|fi| fi.pod_dyn.is_none())
        .collect();

    let fields = static_fields
        .iter()
        .map(|fi| {
            let field = fi.field;
            let vis = &field.vis;
            let name = field.ident.as_ref().expect("field must be named");
            let zc_ty = map_to_pod_type(&field.ty);
            quote! { #vis #name: #zc_ty }
        })
        .collect();

    let schema_fields = static_fields
        .iter()
        .map(|fi| {
            let field = fi.field;
            let vis = &field.vis;
            let name = field.ident.as_ref().expect("field must be named");
            let ty = &field.ty;
            quote! { #vis #name: #ty }
        })
        .collect();

    let zc_name = format_ident!("{}Zc", name);
    let zc_mod = format_ident!("__{}_zc", pascal_to_snake(&name.to_string()));
    let zc_path = if has_dynamic {
        quote! { #zc_name }
    } else {
        quote! { #zc_mod::#zc_name }
    };

    ZcSpec {
        zc_name,
        zc_mod,
        zc_path,
        fields,
        schema_fields,
    }
}

pub(super) fn emit_bump_offset_impl(
    field_infos: &[PodFieldInfo<'_>],
    has_dynamic: bool,
    disc_len: usize,
    zc_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let has_bump_u8 = !has_dynamic
        && field_infos.iter().any(|fi| {
            fi.field.ident.as_ref().is_some_and(|id| id == "bump")
                && matches!(&fi.field.ty, syn::Type::Path(tp) if tp.path.is_ident("u8"))
        });

    if has_bump_u8 {
        quote! {
            const BUMP_OFFSET: Option<usize> = Some(
                #disc_len + core::mem::offset_of!(#zc_path, bump)
            );
        }
    } else {
        quote! {}
    }
}

pub(super) fn emit_zc_definition(
    name: &syn::Ident,
    has_dynamic: bool,
    zc: &ZcSpec,
    align_asserts: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    let zc_name = &zc.zc_name;
    let zc_mod = &zc.zc_mod;
    let zc_fields = &zc.fields;

    if has_dynamic {
        quote! {
            #[repr(C)]
            #[derive(Copy, Clone)]
            pub struct #zc_name {
                #(#zc_fields,)*
            }

            const _: () = assert!(
                core::mem::align_of::<#zc_name>() == 1,
                "ZC companion struct must have alignment 1"
            );

            #(#align_asserts)*

            const _: () = assert!(
                core::mem::size_of::<#name>() == core::mem::size_of::<AccountView>(),
                "Pod-dynamic struct must be #[repr(transparent)] over AccountView"
            );
        }
    } else {
        let schema_fields = &zc.schema_fields;
        quote! {
            #[doc(hidden)]
            pub mod #zc_mod {
                use super::*;
                use quasar_lang::__zeropod as zeropod;

                #[derive(zeropod::ZeroPod)]
                pub struct __Schema {
                    #(#schema_fields,)*
                }

                pub type #zc_name = __SchemaZc;
            }
        }
    }
}

pub(super) fn emit_account_wrapper(
    attrs: &[syn::Attribute],
    vis: &syn::Visibility,
    name: &syn::Ident,
    disc_len: usize,
    zc_path: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        #(#attrs)*
        #[repr(transparent)]
        #vis struct #name {
            __view: AccountView,
        }

        unsafe impl StaticView for #name {}

        impl AsAccountView for #name {
            #[inline(always)]
            fn to_account_view(&self) -> &AccountView {
                &self.__view
            }
        }

        impl core::ops::Deref for #name {
            type Target = #zc_path;

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                unsafe { &*(self.__view.data_ptr().add(#disc_len) as *const #zc_path) }
            }
        }

        impl core::ops::DerefMut for #name {
            #[inline(always)]
            fn deref_mut(&mut self) -> &mut Self::Target {
                unsafe { &mut *(self.__view.data_mut_ptr().add(#disc_len) as *mut #zc_path) }
            }
        }
    }
}
