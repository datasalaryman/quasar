use solana_instruction_view::{
    cpi::{invoke_signed, Signer},
    InstructionAccount, InstructionView,
};
use solana_account_view::AccountView;
use solana_address::Address;
use solana_program_error::ProgramResult;

const SYSTEM_PROGRAM_ID: Address = Address::new_from_array([0u8; 32]);

pub struct CreateAccount<'a, 'b> {
    pub from: &'a AccountView,
    pub to: &'a AccountView,
    pub lamports: u64,
    pub space: u64,
    pub owner: &'b Address,
}

impl CreateAccount<'_, '_> {
    #[inline(always)]
    pub fn invoke(&self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    #[inline(always)]
    pub fn invoke_signed(&self, signers: &[Signer]) -> ProgramResult {
        let instruction_accounts: [InstructionAccount; 2] = [
            InstructionAccount::writable_signer(self.from.address()),
            InstructionAccount::writable_signer(self.to.address()),
        ];

        // [0..4]: discriminator (0 = CreateAccount)
        // [4..12]: lamports
        // [12..20]: space
        // [20..52]: owner
        let mut instruction_data = [0u8; 52];
        instruction_data[4..12].copy_from_slice(&self.lamports.to_le_bytes());
        instruction_data[12..20].copy_from_slice(&self.space.to_le_bytes());
        instruction_data[20..52].copy_from_slice(self.owner.as_ref());

        let instruction = InstructionView {
            program_id: &SYSTEM_PROGRAM_ID,
            accounts: &instruction_accounts,
            data: &instruction_data,
        };

        invoke_signed(&instruction, &[self.from, self.to], signers)
    }
}

pub struct Transfer<'a> {
    pub from: &'a AccountView,
    pub to: &'a AccountView,
    pub lamports: u64,
}

impl Transfer<'_> {
    #[inline(always)]
    pub fn invoke(&self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    #[inline(always)]
    pub fn invoke_signed(&self, signers: &[Signer]) -> ProgramResult {
        let instruction_accounts: [InstructionAccount; 2] = [
            InstructionAccount::writable_signer(self.from.address()),
            InstructionAccount::writable(self.to.address()),
        ];

        // [0..4]: discriminator (2 = Transfer)
        // [4..12]: lamports
        let mut instruction_data = [0u8; 12];
        instruction_data[0] = 2;
        instruction_data[4..12].copy_from_slice(&self.lamports.to_le_bytes());

        let instruction = InstructionView {
            program_id: &SYSTEM_PROGRAM_ID,
            accounts: &instruction_accounts,
            data: &instruction_data,
        };

        invoke_signed(&instruction, &[self.from, self.to], signers)
    }
}

pub struct Assign<'a, 'b> {
    pub account: &'a AccountView,
    pub owner: &'b Address,
}

impl Assign<'_, '_> {
    #[inline(always)]
    pub fn invoke(&self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    #[inline(always)]
    pub fn invoke_signed(&self, signers: &[Signer]) -> ProgramResult {
        let instruction_accounts: [InstructionAccount; 1] =
            [InstructionAccount::writable_signer(self.account.address())];

        // [0..4]: discriminator (1 = Assign)
        // [4..36]: owner
        let mut instruction_data = [0u8; 36];
        instruction_data[0] = 1;
        instruction_data[4..36].copy_from_slice(self.owner.as_ref());

        let instruction = InstructionView {
            program_id: &SYSTEM_PROGRAM_ID,
            accounts: &instruction_accounts,
            data: &instruction_data,
        };

        invoke_signed(&instruction, &[self.account], signers)
    }
}
