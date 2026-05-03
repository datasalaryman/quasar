//! Descriptor registry — Vec + newtype indices with multi-index lookup.
//!
//! Follows the la-arena pattern from rust-analyzer: typed indices into Vec
//! storage.

use {
    crate::ir::{
        descriptor::*,
        identity::{Interner, StableId, StableIdData},
    },
    std::collections::HashMap,
};

/// Typed index into a descriptor Vec.
macro_rules! define_index {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);
    };
}

define_index!(ProgramIdx);
define_index!(InstructionIdx);
define_index!(AccountMetaIdx);
define_index!(AccountDataIdx);
define_index!(TypeIdx);
define_index!(CodecIdx);
define_index!(EventIdx);
define_index!(ErrorIdx);

/// Reference to any descriptor by kind + index.
#[derive(Clone, Copy, Debug)]
pub enum DescriptorRef {
    Program(ProgramIdx),
    Instruction(InstructionIdx),
    AccountMeta(AccountMetaIdx),
    AccountData(AccountDataIdx),
    Type(TypeIdx),
    Codec(CodecIdx),
    Event(EventIdx),
    Error(ErrorIdx),
}

/// The descriptor registry. Stores all descriptors collected from a crate.
#[derive(Default, Debug)]
pub struct DescriptorRegistry {
    pub programs: Vec<ProgramDescriptor>,
    pub instructions: Vec<InstructionDescriptor>,
    pub account_metas: Vec<AccountMetaDescriptor>,
    pub account_data: Vec<AccountDataDescriptor>,
    pub types: Vec<TypeDescriptor>,
    pub codecs: Vec<CodecDescriptor>,
    pub events: Vec<EventDescriptor>,
    pub errors: Vec<ErrorDescriptor>,

    /// Lookup by StableId → DescriptorRef.
    by_id: HashMap<StableId, DescriptorRef>,
    /// Lookup by display name → StableId (may have collisions for different
    /// namespaces).
    by_name: HashMap<String, Vec<StableId>>,
    /// String interner for identity data.
    pub interner: Interner,
    /// Full identity data for diagnostics.
    identity_data: HashMap<StableId, StableIdData>,
}

impl DescriptorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_program(&mut self, desc: ProgramDescriptor) -> ProgramIdx {
        let idx = ProgramIdx(self.programs.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Program(idx));
        self.programs.push(desc);
        idx
    }

    pub fn add_instruction(&mut self, desc: InstructionDescriptor) -> InstructionIdx {
        let idx = InstructionIdx(self.instructions.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Instruction(idx));
        self.instructions.push(desc);
        idx
    }

    pub fn add_account_meta(&mut self, desc: AccountMetaDescriptor) -> AccountMetaIdx {
        let idx = AccountMetaIdx(self.account_metas.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::AccountMeta(idx));
        self.account_metas.push(desc);
        idx
    }

    pub fn add_account_data(&mut self, desc: AccountDataDescriptor) -> AccountDataIdx {
        let idx = AccountDataIdx(self.account_data.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::AccountData(idx));
        self.account_data.push(desc);
        idx
    }

    pub fn add_type(&mut self, desc: TypeDescriptor) -> TypeIdx {
        let idx = TypeIdx(self.types.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Type(idx));
        self.types.push(desc);
        idx
    }

    pub fn add_codec(&mut self, desc: CodecDescriptor) -> CodecIdx {
        let idx = CodecIdx(self.codecs.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Codec(idx));
        self.codecs.push(desc);
        idx
    }

    pub fn add_event(&mut self, desc: EventDescriptor) -> EventIdx {
        let idx = EventIdx(self.events.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Event(idx));
        self.events.push(desc);
        idx
    }

    pub fn add_error(&mut self, desc: ErrorDescriptor) -> ErrorIdx {
        let idx = ErrorIdx(self.errors.len() as u32);
        self.index_descriptor(desc.id, &desc.name, DescriptorRef::Error(idx));
        self.errors.push(desc);
        idx
    }

    /// Look up a descriptor by its StableId.
    pub fn get_by_id(&self, id: StableId) -> Option<DescriptorRef> {
        self.by_id.get(&id).copied()
    }

    /// Look up descriptors by display name (may return multiple across
    /// namespaces).
    pub fn get_by_name(&self, name: &str) -> &[StableId] {
        self.by_name.get(name).map_or(&[], |v| v.as_slice())
    }

    /// Get identity data for diagnostics.
    pub fn get_identity(&self, id: StableId) -> Option<&StableIdData> {
        self.identity_data.get(&id)
    }

    /// Register identity data for a StableId.
    pub fn register_identity(&mut self, id: StableId, data: StableIdData) {
        self.identity_data.insert(id, data);
    }

    fn index_descriptor(&mut self, id: StableId, name: &str, desc_ref: DescriptorRef) {
        self.by_id.insert(id, desc_ref);
        self.by_name.entry(name.to_owned()).or_default().push(id);
    }
}
