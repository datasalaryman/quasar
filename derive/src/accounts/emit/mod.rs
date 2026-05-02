mod output;
pub(crate) mod parse;
mod typed_emit;

use super::resolve::specs::AccountsPlanTyped;
pub(crate) use output::{emit_accounts_output, AccountsOutput};

pub(crate) struct EmitCx {
    pub bumps_name: syn::Ident,
}

pub(crate) fn emit_parse_body(
    semantics: &[super::resolve::FieldSemantics],
    plan: &AccountsPlanTyped,
    cx: &EmitCx,
) -> syn::Result<proc_macro2::TokenStream> {
    parse::emit_parse_body(semantics, plan, cx)
}

pub(crate) fn emit_bump_struct_def(
    semantics: &[super::resolve::FieldSemantics],
    cx: &EmitCx,
) -> proc_macro2::TokenStream {
    parse::emit_bump_struct_def(semantics, cx)
}

pub(crate) fn emit_epilogue(
    semantics: &[super::resolve::FieldSemantics],
    plan: &AccountsPlanTyped,
) -> syn::Result<proc_macro2::TokenStream> {
    parse::emit_epilogue(semantics, plan)
}

pub(crate) fn emit_has_epilogue(
    plan: &AccountsPlanTyped,
    semantics: &[super::resolve::FieldSemantics],
) -> proc_macro2::TokenStream {
    parse::emit_has_epilogue_typed(plan, semantics)
}
