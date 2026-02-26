use pinocchio::error::ProgramError;

#[repr(u32)]
pub enum VaultError {
    InvalidInstructionData = 0,
    NotEnoughAccountKeys = 1,
    MissingRequiredSignature = 2,
    IncorrectSystemProgram = 3,
    InvalidPDA = 4,
}

impl From<VaultError> for ProgramError {
    fn from(e: VaultError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
