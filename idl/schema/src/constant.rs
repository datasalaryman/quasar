use serde::{Deserialize, Serialize};

/// A program constant exposed to clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdlConstant {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub value: IdlConstantValue,
}

/// Tagged constant value representation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum IdlConstantValue {
    #[serde(rename = "integer")]
    Integer { value: String },
    #[serde(rename = "bytes")]
    Bytes { encoding: String, value: String },
    #[serde(rename = "bool")]
    Bool { value: bool },
    #[serde(rename = "string")]
    StringVal { value: String },
    #[serde(rename = "pubkey")]
    Pubkey { value: String },
}
