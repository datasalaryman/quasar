use serde::{Deserialize, Serialize};

/// Root-level wrapper registry. Declares all framework extension points.
/// v1 supports types and codecs only. Other categories reserved for v1.1+.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdlWrappers {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<IdlTypeWrapper>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codecs: Vec<IdlCodecWrapper>,
}

/// A custom type wrapper declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdlTypeWrapper {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

/// A custom codec wrapper declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdlCodecWrapper {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}
