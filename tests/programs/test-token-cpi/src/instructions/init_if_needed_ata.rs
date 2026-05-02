use {quasar_derive::Accounts, quasar_lang::prelude::*, quasar_spl::prelude::*};
#[derive(Accounts)]
pub struct InitIfNeededAta {
    #[account(mut)]
    pub payer: Signer,
    #[account(mut,
        init(idempotent),
        associated_token(authority = wallet, mint = mint, token_program = token_program, system_program = system_program, ata_program = ata_program),
    )]
    pub ata: Account<Token>,
    pub wallet: Signer,
    pub mint: Account<Mint>,
    pub token_program: Program<TokenProgram>,
    pub system_program: Program<SystemProgram>,
    pub ata_program: Program<AssociatedTokenProgram>,
}
impl InitIfNeededAta {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
