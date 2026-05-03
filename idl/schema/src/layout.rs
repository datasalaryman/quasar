use serde::{Deserialize, Serialize};

/// Describes how fields are arranged within a container.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum IdlLayout {
    /// Sequential fixed-size fields with no tail area.
    #[serde(rename = "fixed")]
    Fixed { fields: Vec<String> },
    /// Compact layout: inline fields, then tail headers, then tail payloads.
    #[serde(rename = "compact")]
    Compact {
        #[serde(rename = "inlineFields")]
        inline_fields: Vec<String>,
        #[serde(rename = "tailFields")]
        tail_fields: Vec<String>,
        wire: CompactWire,
    },
}

/// Wire format ordering for compact layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactWire {
    #[serde(rename = "inlineFieldsThenTailHeadersThenTailPayloads")]
    InlineFieldsThenTailHeadersThenTailPayloads,
}
