//! `#[derive(QuasarSerialize)]` — generates instruction-arg impls.
//!
//! **Fixed structs** (no lifetime params, all fields `Copy`):
//! 1. An alignment-1 ZC companion struct `__NameZc`.
//! 2. An `InstructionArg` impl for zero-copy deserialization.
//! 3. Off-chain `SchemaWrite` / `SchemaRead` impls (cfg not-solana).
//!
//! **Borrowed structs** (has lifetime params, fields include `&'a` refs):
//! 1. An `InstructionArgDecode<'a>` impl with a fixed-header batch + sequential
//!    variable-length reads. Reference fields require `#[max(N)]`.
//! 2. No ZC companion or `InstructionArg` impl (not `Copy`).

use {
    proc_macro::TokenStream,
    proc_macro2::TokenStream as TokenStream2,
    quote::{format_ident, quote},
    syn::{
        parse::ParseStream, parse_macro_input, parse_quote, Data, DeriveInput, Field, Fields,
        Ident, LitInt, Token, Type,
    },
};

pub(crate) fn derive_quasar_serialize(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => fields.named.iter().cloned().collect::<Vec<_>>(),
            _ => {
                return syn::Error::new_spanned(
                    &input.ident,
                    "QuasarSerialize can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                &input.ident,
                "QuasarSerialize can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    // Route to borrowed path if any lifetime param is present.
    if input.generics.lifetimes().next().is_some() {
        return derive_borrowed(input, fields);
    }

    derive_fixed(input, fields)
}

// ---------------------------------------------------------------------------
// Fixed struct path (original behaviour)
// ---------------------------------------------------------------------------

fn derive_fixed(input: DeriveInput, fields: Vec<Field>) -> TokenStream {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let zc_name = format_ident!("__{}Zc", name);

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref()).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    let zc_field_types: Vec<_> = field_types
        .iter()
        .map(|ty| {
            quote! { <#ty as quasar_lang::instruction_arg::InstructionArg>::Zc }
        })
        .collect();

    let from_zc_fields: Vec<_> = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! {
                #name: <#ty as quasar_lang::instruction_arg::InstructionArg>::from_zc(&zc.#name)
            }
        })
        .collect();

    let to_zc_fields: Vec<_> = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! {
                #name: <#ty as quasar_lang::instruction_arg::InstructionArg>::to_zc(&self.#name)
            }
        })
        .collect();

    let validate_zc_fields: Vec<_> = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! {
                <#ty as quasar_lang::instruction_arg::InstructionArg>::validate_zc(&zc.#name)?;
            }
        })
        .collect();

    let mut schema_write_generics = input.generics.clone();
    schema_write_generics
        .params
        .push(parse_quote!(__C: wincode::config::ConfigCore));
    let (schema_write_impl_generics, _, _) = schema_write_generics.split_for_impl();

    let mut schema_read_generics = input.generics.clone();
    schema_read_generics.params.insert(0, parse_quote!('__de));
    schema_read_generics
        .params
        .push(parse_quote!(__C: wincode::config::ConfigCore));
    let (schema_read_impl_generics, _, _) = schema_read_generics.split_for_impl();

    let expanded = quote! {
        // Alignment-1 ZC companion for zero-copy instruction deserialization.
        #[doc(hidden)]
        #[repr(C)]
        pub struct #zc_name #generics #where_clause {
            #(#field_names: #zc_field_types,)*
        }

        impl #impl_generics core::marker::Copy for #zc_name #ty_generics #where_clause {}

        impl #impl_generics core::clone::Clone for #zc_name #ty_generics #where_clause {
            #[inline(always)]
            fn clone(&self) -> Self {
                *self
            }
        }

        #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
        unsafe impl #schema_write_impl_generics wincode::SchemaWrite<__C>
            for #zc_name #ty_generics #where_clause
        {
            type Src = Self;

            fn size_of(_src: &Self) -> wincode::error::WriteResult<usize> {
                Ok(core::mem::size_of::<Self>())
            }

            fn write(mut __writer: impl wincode::io::Writer, src: &Self) -> wincode::error::WriteResult<()> {
                let __bytes = unsafe {
                    core::slice::from_raw_parts(
                        src as *const Self as *const u8,
                        core::mem::size_of::<Self>(),
                    )
                };
                __writer.write(__bytes)?;
                Ok(())
            }
        }

        #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
        unsafe impl #schema_read_impl_generics wincode::SchemaRead<'__de, __C>
            for #zc_name #ty_generics #where_clause
        {
            type Dst = Self;

            fn read(
                mut __reader: impl wincode::io::Reader<'__de>,
                __dst: &mut core::mem::MaybeUninit<Self>,
            ) -> wincode::error::ReadResult<()> {
                let __bytes = __reader.take_scoped(core::mem::size_of::<Self>())?;
                let __zc = unsafe { core::ptr::read_unaligned(__bytes.as_ptr() as *const Self) };
                __dst.write(__zc);
                Ok(())
            }
        }

        impl #impl_generics quasar_lang::instruction_arg::InstructionArg
            for #name #ty_generics #where_clause
        {
            type Zc = #zc_name #ty_generics;
            #[inline(always)]
            fn from_zc(zc: &#zc_name #ty_generics) -> Self {
                Self {
                    #(#from_zc_fields,)*
                }
            }
            #[inline(always)]
            fn to_zc(&self) -> #zc_name #ty_generics {
                #zc_name {
                    #(#to_zc_fields,)*
                }
            }
            #[inline(always)]
            fn validate_zc(
                zc: &#zc_name #ty_generics,
            ) -> Result<(), quasar_lang::prelude::ProgramError> {
                #(#validate_zc_fields)*
                Ok(())
            }
        }

        // Wincode SchemaWrite + SchemaRead (off-chain only)
        //
        // Serializes each field via its ZC (zero-copy) representation to
        // guarantee the wire format matches the on-chain ZC layout exactly.
        // This is critical for types like Option<T> where wincode's built-in
        // encoding is variable-length but the on-chain ZC companion (OptionZc)
        // is fixed-size.
        #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
        unsafe impl #schema_write_impl_generics wincode::SchemaWrite<__C>
            for #name #ty_generics #where_clause
        {
            type Src = Self;

            fn size_of(_src: &Self) -> wincode::error::WriteResult<usize> {
                Ok(core::mem::size_of::<#zc_name #ty_generics>())
            }

            fn write(mut __writer: impl wincode::io::Writer, src: &Self) -> wincode::error::WriteResult<()> {
                let __zc = <Self as quasar_lang::instruction_arg::InstructionArg>::to_zc(src);
                let __bytes = unsafe {
                    core::slice::from_raw_parts(
                        &__zc as *const #zc_name #ty_generics as *const u8,
                        core::mem::size_of::<#zc_name #ty_generics>(),
                    )
                };
                __writer.write(__bytes)?;
                Ok(())
            }
        }

        #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
        unsafe impl #schema_read_impl_generics wincode::SchemaRead<'__de, __C>
            for #name #ty_generics #where_clause
        {
            type Dst = Self;

            fn read(
                mut __reader: impl wincode::io::Reader<'__de>,
                __dst: &mut core::mem::MaybeUninit<Self>,
            ) -> wincode::error::ReadResult<()> {
                let __bytes = __reader.take_scoped(core::mem::size_of::<#zc_name #ty_generics>())?;
                let __zc = unsafe { &*(__bytes.as_ptr() as *const #zc_name #ty_generics) };
                __dst.write(<Self as quasar_lang::instruction_arg::InstructionArg>::from_zc(__zc));
                Ok(())
            }
        }
    };

    expanded.into()
}

// ---------------------------------------------------------------------------
// Borrowed struct path (has lifetime params)
// ---------------------------------------------------------------------------

/// Classification of a field in a borrowed struct.
///
/// When adding a new variant, the compiler will force exhaustive handling in
/// `derive_borrowed`'s `match kind` — grep for `FieldKind::Fixed =>`.
enum FieldKind {
    /// Fixed-size non-reference field — goes into the batch ZC header.
    Fixed,
    /// `&'a str` — decoded with `read_dynamic_str`. Requires `#[max(N)]`.
    Str { max_n: usize, pfx: usize },
    /// `&'a [T]` — decoded with `read_dynamic_vec`. Requires `#[max(N)]`.
    Slice {
        elem: Type,
        max_n: usize,
        pfx: usize,
    },
}

impl FieldKind {
    fn is_fixed(&self) -> bool {
        matches!(self, FieldKind::Fixed)
    }
}

/// Parse `#[max(N)]` or `#[max(N, pfx = P)]` from a field's attributes.
/// Returns `Ok(Some((max_n, pfx)))` if found, `Ok(None)` if absent, or
/// `Err` if the attribute is present but malformed.
fn parse_max_attr(field: &Field) -> Result<Option<(usize, usize)>, syn::Error> {
    for attr in &field.attrs {
        if attr.path().is_ident("max") {
            let pair = attr.parse_args_with(|stream: ParseStream| {
                let n: LitInt = stream.parse()?;
                let max_n: usize = n
                    .base10_parse()
                    .map_err(|e| syn::Error::new(n.span(), e.to_string()))?;
                let mut pfx = 0usize; // 0 = use type-specific default
                if !stream.is_empty() {
                    let _: Token![,] = stream.parse()?;
                    let key: Ident = stream.parse()?;
                    if key != "pfx" {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown #[max] option `{key}`, expected `pfx`"),
                        ));
                    }
                    let _: Token![=] = stream.parse()?;
                    let p: LitInt = stream.parse()?;
                    pfx = p
                        .base10_parse()
                        .map_err(|e| syn::Error::new(p.span(), e.to_string()))?;
                    if !matches!(pfx, 1 | 2 | 4 | 8) {
                        return Err(syn::Error::new(p.span(), "pfx must be 1, 2, 4, or 8"));
                    }
                }
                Ok((max_n, pfx))
            })?;
            return Ok(Some(pair));
        }
    }
    Ok(None)
}

/// Classify a field in a borrowed struct.
fn classify_field(field: &Field) -> Result<FieldKind, syn::Error> {
    if let Type::Reference(ref_ty) = &field.ty {
        let is_str = matches!(&*ref_ty.elem, Type::Path(tp) if tp.path.is_ident("str"));
        let slice_elem: Option<Type> = if let Type::Slice(s) = &*ref_ty.elem {
            Some((*s.elem).clone())
        } else {
            None
        };

        if !is_str && slice_elem.is_none() {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "QuasarSerialize: reference fields must be `&'a str` or `&'a [T]`",
            ));
        }

        let name_str = field
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_default();

        let (max_n, pfx_override) = parse_max_attr(field)?.ok_or_else(|| {
            syn::Error::new_spanned(
                &field.ty,
                format!(
                    "QuasarSerialize: reference field `{}` requires `#[max(N)]`",
                    name_str
                ),
            )
        })?;

        if is_str {
            let pfx = if pfx_override == 0 { 1 } else { pfx_override };
            Ok(FieldKind::Str { max_n, pfx })
        } else {
            let pfx = if pfx_override == 0 { 2 } else { pfx_override };
            Ok(FieldKind::Slice {
                elem: slice_elem.unwrap(),
                max_n,
                pfx,
            })
        }
    } else {
        Ok(FieldKind::Fixed)
    }
}

fn derive_borrowed(input: DeriveInput, fields: Vec<Field>) -> TokenStream {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Classify each field.
    let mut kinds: Vec<FieldKind> = Vec::with_capacity(fields.len());
    for field in &fields {
        match classify_field(field) {
            Ok(k) => kinds.push(k),
            Err(e) => return e.to_compile_error().into(),
        }
    }

    // Fixed fields → batch ZC header.
    let fixed_fields: Vec<_> = fields
        .iter()
        .zip(kinds.iter())
        .filter(|(_, k)| k.is_fixed())
        .map(|(f, _)| f)
        .collect();

    let fixed_names: Vec<_> = fixed_fields
        .iter()
        .map(|f| f.ident.as_ref().unwrap())
        .collect();
    let fixed_types: Vec<_> = fixed_fields.iter().map(|f| &f.ty).collect();

    // Build batch header struct + decode block (empty if no fixed fields).
    let header_struct: TokenStream2 = if !fixed_fields.is_empty() {
        let fixed_zc_types: Vec<_> = fixed_types
            .iter()
            .map(|ty| quote! { <#ty as quasar_lang::instruction_arg::InstructionArg>::Zc })
            .collect();
        quote! {
            #[repr(C)]
            struct __FixedHeader {
                #(#fixed_names: #fixed_zc_types,)*
            }
            const _: () = assert!(
                core::mem::align_of::<__FixedHeader>() == 1,
                "QuasarSerialize borrowed: all fixed fields must have alignment-1 Zc types"
            );
        }
    } else {
        quote! {}
    };

    let header_decode: TokenStream2 = if !fixed_fields.is_empty() {
        let validate_stmts: Vec<_> = fixed_names
            .iter()
            .zip(fixed_types.iter())
            .map(|(fname, ty)| {
                quote! {
                    <#ty as quasar_lang::instruction_arg::InstructionArg>::validate_zc(
                        &__hdr.#fname
                    )?;
                }
            })
            .collect();
        let from_zc_stmts: Vec<_> = fixed_names
            .iter()
            .zip(fixed_types.iter())
            .map(|(fname, ty)| {
                quote! {
                    let #fname =
                        <#ty as quasar_lang::instruction_arg::InstructionArg>::from_zc(
                            &__hdr.#fname
                        );
                }
            })
            .collect();
        quote! {
            if data.len() < offset + core::mem::size_of::<__FixedHeader>() {
                return Err(quasar_lang::prelude::ProgramError::InvalidInstructionData);
            }
            let __hdr = unsafe { &*(data.as_ptr().add(offset) as *const __FixedHeader) };
            #(#validate_stmts)*
            #(#from_zc_stmts)*
            let mut __offset = offset + core::mem::size_of::<__FixedHeader>();
        }
    } else {
        quote! { let mut __offset = offset; }
    };

    // Variable-field decode statements (in struct field order).
    let mut var_stmts: Vec<TokenStream2> = Vec::new();
    for (field, kind) in fields.iter().zip(kinds.iter()) {
        let fname = field.ident.as_ref().unwrap();
        match kind {
            FieldKind::Fixed => {} // already decoded in header_decode
            FieldKind::Str { max_n, pfx } => {
                var_stmts.push(quote! {
                    let (#fname, __new_offset) =
                        quasar_lang::instruction_data::read_dynamic_str::<#pfx>(
                            data, __offset, #max_n
                        )?;
                    __offset = __new_offset;
                });
            }
            FieldKind::Slice { elem, max_n, pfx } => {
                var_stmts.push(quote! {
                    let (#fname, __new_offset) =
                        quasar_lang::instruction_data::read_dynamic_vec::<#elem, #pfx>(
                            data, __offset, #max_n
                        )?;
                    __offset = __new_offset;
                });
            }
        }
    }

    // Collect all field names for struct construction.
    let all_field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();

    // Use the first lifetime param as the trait's 'a.
    let first_lt = generics.lifetimes().next().map(|ld| &ld.lifetime).unwrap(); // guaranteed: has_lifetime check in caller

    let expanded = quote! {
        impl #impl_generics quasar_lang::instruction_arg::InstructionArgDecode<#first_lt>
            for #name #ty_generics #where_clause
        {
            type Output = Self;

            #[inline(always)]
            fn decode(
                data: &#first_lt [u8],
                offset: usize,
            ) -> Result<(Self, usize), quasar_lang::prelude::ProgramError> {
                #header_struct
                #header_decode
                #(#var_stmts)*
                Ok((Self { #(#all_field_names,)* }, __offset))
            }
        }
    };

    expanded.into()
}
