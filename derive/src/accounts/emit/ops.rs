//! Arg transforms for capability trait context struct construction.
//!
//! The derive emits direct capability trait calls. These helpers transform
//! group directive args into the correct expressions for each phase.

use {
    super::super::resolve::GroupArg,
    quote::quote,
};

/// Context for op emission — carries field names for transform disambiguation.
pub(crate) struct OpEmitCtx {
    pub field_names: Vec<String>,
}

fn is_field_ident(expr: &syn::Expr, ctx: &OpEmitCtx) -> bool {
    if let syn::Expr::Path(ep) = expr {
        if ep.qself.is_none() && ep.path.segments.len() == 1 {
            let name = ep.path.segments[0].ident.to_string();
            return ctx.field_names.iter().any(|f| f == &name);
        }
    }
    false
}

/// Transform arg value for Phase 3 (post-load): field idents get
/// `.to_account_view()`, `Some(field)` becomes `Some(field.to_account_view())`,
/// non-field idents, `None`, and literals pass through.
pub(crate) fn typed_arg(arg: &GroupArg, ctx: &OpEmitCtx) -> proc_macro2::TokenStream {
    transform_typed_expr(&arg.value, ctx)
}

fn transform_typed_expr(expr: &syn::Expr, ctx: &OpEmitCtx) -> proc_macro2::TokenStream {
    match expr {
        // None → pass through
        syn::Expr::Path(ep)
            if ep.qself.is_none()
                && ep.path.segments.len() == 1
                && ep.path.segments[0].ident == "None" =>
        {
            quote! { None }
        }
        // Field ident → typed ref via to_account_view()
        _ if is_field_ident(expr, ctx) => {
            quote! { #expr.to_account_view() }
        }
        // Some(inner) → transform inner recursively, wrap in Some()
        syn::Expr::Call(call)
            if matches!(&*call.func, syn::Expr::Path(p)
                if p.path.segments.len() == 1 && p.path.segments[0].ident == "Some")
                && call.args.len() == 1 =>
        {
            let inner = transform_typed_expr(&call.args[0], ctx);
            quote! { Some(#inner) }
        }
        // Everything else (literals, consts, multi-segment paths) → pass through
        _ => {
            quote! { #expr }
        }
    }
}

/// Transform arg value for Phase 4 (exit): field idents get
/// `self.field.to_account_view()`, `Some(field)` becomes
/// `Some(self.field.to_account_view())`, non-field values pass through.
pub(crate) fn exit_arg(arg: &GroupArg, ctx: &OpEmitCtx) -> proc_macro2::TokenStream {
    transform_exit_expr(&arg.value, ctx)
}

fn transform_exit_expr(expr: &syn::Expr, ctx: &OpEmitCtx) -> proc_macro2::TokenStream {
    match expr {
        syn::Expr::Path(ep)
            if ep.qself.is_none()
                && ep.path.segments.len() == 1
                && ep.path.segments[0].ident == "None" =>
        {
            quote! { None }
        }
        // Field ident → self.field.to_account_view()
        _ if is_field_ident(expr, ctx) => {
            if let syn::Expr::Path(ep) = expr {
                let ident = &ep.path.segments[0].ident;
                quote! { self.#ident.to_account_view() }
            } else {
                unreachable!()
            }
        }
        syn::Expr::Call(call)
            if matches!(&*call.func, syn::Expr::Path(p)
                if p.path.segments.len() == 1 && p.path.segments[0].ident == "Some")
                && call.args.len() == 1 =>
        {
            let inner = transform_exit_expr(&call.args[0], ctx);
            quote! { Some(#inner) }
        }
        // Everything else → pass through
        _ => {
            quote! { #expr }
        }
    }
}
