use quasar_lang::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ReturnPayload {
    pub amount: u64,
    pub flag: bool,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ReturnPayloadZc {
    pub amount: <u64 as InstructionArg>::Zc,
    pub flag: <bool as InstructionArg>::Zc,
}

impl InstructionArg for ReturnPayload {
    type Zc = ReturnPayloadZc;

    #[inline(always)]
    fn from_zc(zc: &Self::Zc) -> Self {
        Self {
            amount: <u64 as InstructionArg>::from_zc(&zc.amount),
            flag: <bool as InstructionArg>::from_zc(&zc.flag),
        }
    }

    #[inline(always)]
    fn to_zc(&self) -> Self::Zc {
        ReturnPayloadZc {
            amount: <u64 as InstructionArg>::to_zc(&self.amount),
            flag: <bool as InstructionArg>::to_zc(&self.flag),
        }
    }
}

pub const RETURN_U64_VALUE: u64 = 777;
pub const RETURN_PAYLOAD_VALUE: ReturnPayload = ReturnPayload {
    amount: 55,
    flag: true,
};

pub struct TestMiscProgram;

impl Id for TestMiscProgram {
    const ID: Address = crate::ID;
}

#[account(discriminator = 1)]
pub struct SimpleAccount {
    pub authority: Address,
    pub value: u64,
    pub bump: u8,
}

#[account(discriminator = [1, 2])]
pub struct MultiDiscAccount {
    pub data: u64,
}

#[account(discriminator = 5)]
pub struct DynamicAccount<'a> {
    pub name: String<u32, 8>,
    pub tags: Vec<Address, u32, 2>,
}

#[account(discriminator = 6)]
pub struct MixedAccount<'a> {
    pub authority: Address,
    pub value: u64,
    pub label: String<u32, 32>,
}

#[account(discriminator = 7)]
pub struct SmallPrefixAccount<'a> {
    pub tag: String<u8, 100>,
    pub scores: Vec<u8, u8, 10>,
}

#[account(discriminator = 8)]
pub struct TailStrAccount<'a> {
    pub authority: Address,
    pub label: &'a str,
}

#[account(discriminator = 9)]
pub struct TailBytesAccount<'a> {
    pub authority: Address,
    pub data: &'a [u8],
}

/// Account with no discriminator — size-only validation.
#[account(unsafe_no_disc)]
pub struct NoDiscAccount {
    pub authority: Address,
    pub value: u64,
}
