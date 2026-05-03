//! The 9 descriptor family types for v1.
//!
//! Each descriptor represents a semantic fact emitted by a derive macro
//! or manual trait impl. Descriptors are unresolved — they contain `TypeRef`
//! and `CodecRef` that must go through the resolution pipeline.

use crate::ir::{
    identity::StableId,
    refs::{CodecRef, PrimitiveType, StorageRef, TypeRef},
};

/// Top-level enum of all descriptor kinds. Closed set — exhaustive matching.
#[derive(Clone, Debug)]
pub enum DescriptorKind {
    Program(ProgramDescriptor),
    Instruction(InstructionDescriptor),
    AccountMeta(AccountMetaDescriptor),
    AccountData(AccountDataDescriptor),
    Type(TypeDescriptor),
    Codec(CodecDescriptor),
    Event(EventDescriptor),
    Error(ErrorDescriptor),
}

/// The program itself.
#[derive(Clone, Debug)]
pub struct ProgramDescriptor {
    pub id: StableId,
    pub name: String,
    pub address: String,
    pub version: String,
    pub instructions: Vec<StableId>,
    pub accounts: Vec<StableId>,
    pub events: Vec<StableId>,
    pub errors: Vec<StableId>,
    pub types: Vec<StableId>,
}

/// An instruction handler.
#[derive(Clone, Debug)]
pub struct InstructionDescriptor {
    pub id: StableId,
    pub name: String,
    pub discriminator: Vec<u8>,
    pub accounts_struct: StableId,
    pub args: Vec<FieldDescriptor>,
    pub has_remaining: bool,
    pub returns: Option<TypeRef>,
    pub docs: Vec<String>,
}

/// An account field within a #[derive(Accounts)] struct.
#[derive(Clone, Debug)]
pub struct AccountMetaDescriptor {
    pub id: StableId,
    /// The accounts struct this belongs to (instruction identity).
    pub instruction_id: StableId,
    pub name: String,
    pub writable: bool,
    pub signer: bool,
    pub optional: bool,
    pub resolver: ResolverInfo,
    pub docs: Vec<String>,
}

/// Resolver info for an account meta (folded into AccountMetaDescriptor for
/// v1).
#[derive(Clone, Debug)]
pub enum ResolverInfo {
    Input,
    Const {
        address: String,
    },
    KnownProgram {
        name: String,
    },
    Pda {
        seeds: Vec<SeedInfo>,
        bump: BumpInfo,
    },
    AssociatedToken {
        mint: String,
        owner: String,
    },
    Arg {
        path: String,
    },
}

/// A PDA seed descriptor.
#[derive(Clone, Debug)]
pub enum SeedInfo {
    Const(Vec<u8>),
    Account { path: String },
    Arg { path: String, ty: PrimitiveType },
}

/// How the PDA bump is sourced.
#[derive(Clone, Debug)]
pub enum BumpInfo {
    Canonical,
    Arg { path: String },
}

/// An account data type (state stored on-chain).
#[derive(Clone, Debug)]
pub struct AccountDataDescriptor {
    pub id: StableId,
    pub name: String,
    pub discriminator: Vec<u8>,
    pub type_id: StableId,
    pub space_min: u64,
    pub space_max: Option<u64>,
    pub docs: Vec<String>,
}

/// A type definition (struct, enum, alias, etc.).
#[derive(Clone, Debug)]
pub struct TypeDescriptor {
    pub id: StableId,
    pub name: String,
    pub kind: TypeKind,
    pub fields: Vec<FieldDescriptor>,
    pub variants: Vec<VariantDescriptor>,
    pub generics: Vec<GenericParamDescriptor>,
    pub alias_target: Option<TypeRef>,
    pub docs: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    TupleStruct,
    Alias,
    Enum,
    Opaque,
}

/// A field within a type or instruction args.
#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    pub name: String,
    pub type_ref: TypeRef,
    pub codec_ref: CodecRef,
    pub storage: Option<StorageRef>,
    pub docs: Vec<String>,
}

/// An enum variant.
#[derive(Clone, Debug)]
pub struct VariantDescriptor {
    pub name: String,
    pub value: u64,
    pub fields: Vec<FieldDescriptor>,
}

/// A generic parameter.
#[derive(Clone, Debug)]
pub struct GenericParamDescriptor {
    pub name: String,
    pub is_const: bool,
    pub const_type: Option<String>,
}

/// A codec descriptor (explicit codec info for a type).
#[derive(Clone, Debug)]
pub struct CodecDescriptor {
    pub id: StableId,
    pub name: String,
    pub codec_ref: CodecRef,
}

/// An event descriptor.
#[derive(Clone, Debug)]
pub struct EventDescriptor {
    pub id: StableId,
    pub name: String,
    pub discriminator: Vec<u8>,
    pub type_id: StableId,
    pub docs: Vec<String>,
}

/// An error descriptor.
#[derive(Clone, Debug)]
pub struct ErrorDescriptor {
    pub id: StableId,
    pub code: u32,
    pub name: String,
    pub msg: Option<String>,
}
