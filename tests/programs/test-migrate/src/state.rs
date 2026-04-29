use quasar_lang::prelude::*;

// ---------------------------------------------------------------------------
// V1: original layout (disc = 1)
// ---------------------------------------------------------------------------

#[account(discriminator = 1)]
pub struct ConfigV1 {
    pub authority: Address,
    pub value: PodU64,
}

// ---------------------------------------------------------------------------
// V2: larger layout (disc = 2) — adds a new field
// ---------------------------------------------------------------------------

#[account(discriminator = 2)]
pub struct ConfigV2 {
    pub authority: Address,
    pub value: PodU64,
    pub extra: PodU32,
}

// ---------------------------------------------------------------------------
// Migration: V1 → V2
// ---------------------------------------------------------------------------

impl Migrate<ConfigV2Data> for ConfigV1Data {
    fn migrate(&self) -> ConfigV2Data {
        ConfigV2Data {
            authority: self.authority,
            value: self.value,
            extra: PodU32::from(42),
        }
    }
}
