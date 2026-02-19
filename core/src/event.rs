use solana_account_view::AccountView;
use solana_program_error::ProgramError;
use crate::cpi::{invoke_signed_unchecked, CpiAccount, InstructionView, InstructionAccount, Signer, Seed};

#[inline(always)]
pub fn emit_event_cpi(
    program: &AccountView,
    event_authority: &AccountView,
    instruction_data: &[u8],
    bump: u8,
) -> Result<(), ProgramError> {
    let instruction = InstructionView {
        program_id: program.address(),
        accounts: &[InstructionAccount::readonly_signer(event_authority.address())],
        data: instruction_data,
    };

    let bump_ref = [bump];
    let seeds = [
        Seed::from(b"__event_authority" as &[u8]),
        Seed::from(&bump_ref as &[u8]),
    ];
    let signer = Signer::from(&seeds as &[Seed]);
    let cpi_account = CpiAccount::from(event_authority);

    unsafe {
        invoke_signed_unchecked(
            &instruction,
            &[cpi_account],
            &[signer],
        )
    };
    Ok(())
}
