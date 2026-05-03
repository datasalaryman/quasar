//! Unresolved references used in descriptors.
//!
//! These get resolved into concrete types during the resolution pipeline.

use crate::ir::identity::StableId;

/// An unresolved type reference. Macros emit these; the resolver resolves them.
#[derive(Clone, Debug)]
pub enum TypeRef {
    /// Primitive type (e.g., "u64", "pubkey", "bool").
    Primitive(PrimitiveType),
    /// Path to a named type (e.g., "my_crate::types::Config").
    Path {
        module_path: Vec<String>,
        name: String,
    },
    /// Reference by stable ID (already resolved in a prior pass).
    Id(StableId),
    /// Option wrapping another type.
    Option(Box<TypeRef>),
    /// Vec of another type.
    Vec(Box<TypeRef>),
    /// Fixed-size array.
    Array(Box<TypeRef>, usize),
    /// Generic type parameter (unresolved).
    Generic(String),
    /// Inline defined type (for anonymous structs in args).
    Inline(Box<super::descriptor::TypeDescriptor>),
}

/// Well-known primitive types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Pubkey,
    Bytes,
    String,
}

impl PrimitiveType {
    /// Parse from string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bool" => Some(Self::Bool),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "u128" => Some(Self::U128),
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "i128" => Some(Self::I128),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            "pubkey" => Some(Self::Pubkey),
            "bytes" => Some(Self::Bytes),
            "string" => Some(Self::String),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Pubkey => "pubkey",
            Self::Bytes => "bytes",
            Self::String => "string",
        }
    }

    /// Fixed byte size for this primitive (None for dynamic types like
    /// string/bytes).
    pub fn fixed_size(self) -> Option<usize> {
        match self {
            Self::Bool | Self::U8 | Self::I8 => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::U64 | Self::I64 | Self::F64 => Some(8),
            Self::U128 | Self::I128 => Some(16),
            Self::Pubkey => Some(32),
            Self::Bytes | Self::String => None,
        }
    }
}

/// An unresolved codec reference.
#[derive(Clone, Debug)]
pub enum CodecRef {
    /// Infer codec from the type (default behavior for scalars, pubkeys, etc.).
    Infer,
    /// Explicit size-prefixed codec for strings.
    SizePrefixedString {
        prefix_bytes: usize,
        max_bytes: usize,
        storage: StorageRef,
    },
    /// Explicit size-prefixed codec for vecs.
    SizePrefixedVec {
        prefix_bytes: usize,
        max_items: usize,
        storage: StorageRef,
        item_type: TypeRef,
    },
    /// Remainder codec (final tail field).
    Remainder { storage: StorageRef },
    /// Fixed bytes codec.
    FixedBytes { size: usize },
}

/// Storage location reference (inline or tail).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageRef {
    Inline,
    Tail,
}
