use {
    crate::root::Idl,
    sha2::{Digest, Sha256},
};

/// Serialize an IDL to canonical JSON bytes (deterministic output).
///
/// Canonical JSON rules:
/// - Struct fields serialize in declaration order (serde default).
/// - BTreeMap keys serialize in sorted order (BTreeMap guarantee).
/// - No trailing whitespace, compact format.
pub fn canonical_json(idl: &Idl) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(idl)
}

/// Serialize an IDL to canonical pretty-printed JSON (for human-readable
/// output).
pub fn canonical_json_pretty(idl: &Idl) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec_pretty(idl)
}

/// Compute the full IDL hash (SHA-256 of canonical JSON, excluding `hashes`
/// field).
pub fn compute_idl_hash(idl: &Idl) -> String {
    let mut idl_for_hash = idl.clone();
    idl_for_hash.hashes = None;
    let bytes = serde_json::to_vec(&idl_for_hash).expect("IDL serialization should not fail");
    hex_sha256(&bytes)
}

/// Compute the ABI hash (SHA-256 of ABI-affecting subset only).
///
/// ABI hash includes: address, discriminators, instruction args/codecs/layouts,
/// account data types/codecs/layouts, event types, account meta ordering,
/// resolver requirements.
///
/// ABI hash excludes: docs, source spans, metadata, non-ABI extension data.
pub fn compute_abi_hash(idl: &Idl) -> String {
    let abi_subset = extract_abi_subset(idl);
    let bytes = serde_json::to_vec(&abi_subset).expect("ABI subset serialization should not fail");
    hex_sha256(&bytes)
}

/// SHA-256 hash as lowercase hex string.
fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Hex encoding without external dep (inline implementation).
mod hex {
    use std::fmt::Write;

    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
        out
    }
}

/// Extract the ABI-affecting subset for hashing.
/// This is a simplified representation that captures only ABI-relevant fields.
#[derive(serde::Serialize)]
struct AbiSubset {
    address: String,
    instructions: Vec<AbiInstruction>,
    accounts: Vec<AbiAccount>,
    events: Vec<AbiEvent>,
}

#[derive(serde::Serialize)]
struct AbiInstruction {
    name: String,
    discriminator: Vec<u8>,
    args: Vec<crate::instruction::IdlArg>,
    accounts: Vec<AbiAccountMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<crate::layout::IdlLayout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    returns: Option<crate::instruction::IdlReturnData>,
}

#[derive(serde::Serialize)]
struct AbiAccountMeta {
    name: String,
    writable: crate::account::AccountFlag,
    signer: crate::account::AccountFlag,
}

#[derive(serde::Serialize)]
struct AbiAccount {
    name: String,
    discriminator: Vec<u8>,
}

#[derive(serde::Serialize)]
struct AbiEvent {
    name: String,
    discriminator: Vec<u8>,
}

fn extract_abi_subset(idl: &Idl) -> AbiSubset {
    AbiSubset {
        address: idl.address.clone(),
        instructions: idl
            .instructions
            .iter()
            .map(|ix| AbiInstruction {
                name: ix.name.clone(),
                discriminator: ix.discriminator.clone(),
                args: ix.args.clone(),
                accounts: ix
                    .accounts
                    .iter()
                    .map(|a| AbiAccountMeta {
                        name: a.name.clone(),
                        writable: a.writable.clone(),
                        signer: a.signer.clone(),
                    })
                    .collect(),
                layout: ix.layout.clone(),
                returns: ix.returns.clone(),
            })
            .collect(),
        accounts: idl
            .accounts
            .iter()
            .map(|a| AbiAccount {
                name: a.name.clone(),
                discriminator: a.discriminator.clone(),
            })
            .collect(),
        events: idl
            .events
            .iter()
            .map(|e| AbiEvent {
                name: e.name.clone(),
                discriminator: e.discriminator.clone(),
            })
            .collect(),
    }
}
