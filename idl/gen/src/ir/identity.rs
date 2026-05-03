//! Stable identity for descriptors.
//!
//! Each descriptor has a `StableId` — a pre-computed 128-bit hash of its
//! fully-qualified identity (package + module path + name + generics).
//! This gives O(1) equality and is `Copy`.

use std::{collections::HashMap, hash::Hash};

/// A stable, content-addressed identity for any descriptor.
/// Pre-computed hash for O(1) equality checks. Copy-friendly (16 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableId(u128);

impl StableId {
    /// Create a StableId from identity components.
    pub fn from_parts(
        package: &str,
        module_path: &[&str],
        name: &str,
        generics: &[StableId],
    ) -> Self {
        let mut hasher = SimpleHasher::new();
        hasher.write_str(package);
        for m in module_path {
            hasher.write_str(m);
        }
        hasher.write_str(name);
        for g in generics {
            hasher.write_u128(g.0);
        }
        Self(hasher.finish_u128())
    }

    /// Create from raw hash value (for deserialization).
    pub fn from_raw(value: u128) -> Self {
        Self(value)
    }

    /// Get the raw hash value.
    pub fn raw(self) -> u128 {
        self.0
    }
}

impl std::fmt::Debug for StableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StableId({:032x})", self.0)
    }
}

/// Full identity data, stored in the interner for display/diagnostics.
#[derive(Clone, Debug)]
pub struct StableIdData {
    pub package: InternedStr,
    pub module_path: Vec<InternedStr>,
    pub name: InternedStr,
    pub generics: Vec<StableId>,
}

/// Index into the string interner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InternedStr(u32);

/// A simple string interner. Deduplicates strings and provides O(1) lookup.
#[derive(Default, Debug)]
pub struct Interner {
    strings: Vec<String>,
    map: HashMap<String, InternedStr>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> InternedStr {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = InternedStr(self.strings.len() as u32);
        self.strings.push(s.to_owned());
        self.map.insert(s.to_owned(), id);
        id
    }

    pub fn resolve(&self, id: InternedStr) -> &str {
        &self.strings[id.0 as usize]
    }
}

/// Simple non-cryptographic hasher for StableId computation.
/// Uses FNV-1a style mixing to produce a 128-bit hash.
struct SimpleHasher {
    state: u128,
}

impl SimpleHasher {
    fn new() -> Self {
        // FNV-1a 128-bit offset basis
        Self {
            state: 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d,
        }
    }

    fn write_str(&mut self, s: &str) {
        for byte in s.bytes() {
            self.state ^= byte as u128;
            self.state = self
                .state
                .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
        // Separator to prevent "ab" + "c" == "a" + "bc"
        self.state ^= 0xff;
        self.state = self
            .state
            .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }

    fn write_u128(&mut self, v: u128) {
        for byte in v.to_le_bytes() {
            self.state ^= byte as u128;
            self.state = self
                .state
                .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
    }

    fn finish_u128(self) -> u128 {
        self.state
    }
}
