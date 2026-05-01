use syn::{Expr, Ident, Type};

/// 2 variants. No domain knowledge.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Single,
    Composite,
}

/// Op classification for direct capability dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OpKind {
    /// Validation only (after typed load).
    Constraint,
    /// Constraint + init param contribution.
    ConstraintAndInit,
    /// Epilogue action (exit phase).
    Exit,
}

pub(crate) struct FieldCore {
    pub ident: Ident,
    pub field: syn::Field,
    pub effective_ty: Type,
    pub kind: FieldKind,
    /// Inner/source type for generic wrappers.
    pub inner_ty: Option<Type>,
    pub optional: bool,
    pub dynamic: bool,
    pub is_mut: bool,
    pub dup: bool,
}

/// A group directive: `path(key = value, ...)`.
#[derive(Clone)]
pub(crate) struct GroupDirective {
    pub path: syn::Path,
    pub args: Vec<GroupArg>,
}

/// A single `key = value` arg in a group directive.
#[derive(Clone)]
pub(crate) struct GroupArg {
    pub key: Ident,
    pub value: Expr,
}

/// User-specified structural assertion.
pub(crate) enum UserCheck {
    HasOne {
        targets: Vec<Ident>,
        error: Option<Expr>,
    },
    Address {
        expr: Expr,
        error: Option<Expr>,
    },
    Constraints {
        exprs: Vec<Expr>,
        error: Option<Expr>,
    },
}

pub(crate) struct FieldSemantics {
    pub core: FieldCore,
    /// `init` / `init(idempotent)` — structural, Phase 1.
    pub init: Option<InitDirective>,
    /// Top-level `payer = field`.
    pub payer: Option<Ident>,
    /// `address = expr` — opaque address constraint.
    pub address: Option<Expr>,
    /// `realloc = expr` — realloc size expression.
    pub realloc: Option<Expr>,
    /// All op groups (used for legacy AccountOp dispatch until fully migrated).
    pub groups: Vec<GroupDirective>,
    /// Constraint ops (Constraint + ConstraintAndInit): run after load.
    pub constraints: Vec<GroupDirective>,
    /// Init contributor ops (ConstraintAndInit only): run during init phase.
    /// Only populated when `has_init()`.
    pub init_contributors: Vec<GroupDirective>,
    /// Exit action ops (Exit kind): run in epilogue.
    /// Sorted: sweep before close/close_program.
    pub exit_actions: Vec<GroupDirective>,
    /// Structural assertions: has_one, address, constraints.
    pub user_checks: Vec<UserCheck>,
}

impl FieldSemantics {
    pub fn has_init(&self) -> bool {
        self.init.is_some()
    }

    pub fn is_writable(&self) -> bool {
        self.core.is_mut || self.has_init()
    }
}

/// Parsed `init` / `init(idempotent)` directive.
pub(crate) struct InitDirective {
    pub idempotent: bool,
}
