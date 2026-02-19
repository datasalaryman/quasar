use alloc::vec;
use mollusk_svm::{Mollusk, program::{create_program_account_loader_v3, create_program_account_pair_loader_v3, keyed_account_for_system_program}};

use solana_address::Address;
use solana_account::Account;
use solana_instruction::Instruction;
use solana_program_pack::Pack;
use spl_token_interface::state::Account as TokenAccount;

use crate::client::MakeInstruction;

#[test]
fn test_make() {
    let mut mollusk = Mollusk::new(&crate::ID, "../../target/deploy/quasar_escrow");

    mollusk_svm_programs_token::token::add_program(&mut mollusk);

    let (token_program, token_program_account) = mollusk_svm_programs_token::token::keyed_account();
    let (system_program, system_program_account) = keyed_account_for_system_program();

    let (maker, maker_account) = (Address::new_unique(), Account::new(1_000_000_000, 0, &system_program));
    let (escrow, escrow_account) = (Address::find_program_address(&[b"escrow", maker.as_ref()], &crate::ID).0, Account::default());

    let mint_a = Address::new_unique();
    let mint_b = Address::new_unique();
    
    let maker_ta_a_token = TokenAccount {
        mint: mint_a,
        owner: maker,
        amount: 1_000_000,
        delegate: None.into(),
        state: spl_token_interface::state::AccountState::Initialized,
        is_native: None.into(),
        delegated_amount: 0,
        close_authority: None.into(),
    };
    let mut maker_ta_a_data = vec![0u8; TokenAccount::LEN];
    Pack::pack(maker_ta_a_token, &mut maker_ta_a_data).unwrap();

    let maker_ta_b_token = TokenAccount {
        mint: mint_b,
        owner: maker,
        amount: 0,
        delegate: None.into(),
        state: spl_token_interface::state::AccountState::Initialized,
        is_native: None.into(),
        delegated_amount: 0,
        close_authority: None.into(),
    };
    let mut maker_ta_b_data = vec![0u8; TokenAccount::LEN];
    Pack::pack(maker_ta_b_token, &mut maker_ta_b_data).unwrap();

    let vault_ta_a_token = TokenAccount {
        mint: mint_a,
        owner: escrow,
        amount: 0,
        delegate: None.into(),
        state: spl_token_interface::state::AccountState::Initialized,
        is_native: None.into(),
        delegated_amount: 0,
        close_authority: None.into(),
    };
    let mut vault_ta_a_data = vec![0u8; TokenAccount::LEN];
    Pack::pack(vault_ta_a_token, &mut vault_ta_a_data).unwrap();

    let (maker_ta_a, maker_ta_a_account) = (Address::new_unique(), Account { lamports: 1_000_000, data: maker_ta_a_data, owner: token_program, executable: false, rent_epoch: 0 });
    let (maker_ta_b, maker_ta_b_account) = (Address::new_unique(), Account { lamports: 1_000_000, data: maker_ta_b_data, owner: token_program, executable: false, rent_epoch: 0 });
    let (vault_ta_a, vault_ta_a_account) = (Address::new_unique(), Account { lamports: 1_000_000, data: vault_ta_a_data, owner: token_program, executable: false, rent_epoch: 0 });
    let (rent, rent_account) = mollusk.sysvars.keyed_account_for_rent_sysvar();
    
    let (event_authority, event_authority_account) = (crate::EventAuthority::ADDRESS, Account::default());
    let (program, program_account) = (crate::ID, create_program_account_loader_v3(&crate::ID.into()));

    let instruction: Instruction = MakeInstruction {
        maker,
        escrow,
        maker_ta_a,
        maker_ta_b,
        vault_ta_a,
        rent,
        token_program,
        system_program,
        event_authority,
        program,
        deposit: 1337,
        receive: 1337,
    }.into();

    let accounts = &[
        (maker, maker_account),
        (escrow, escrow_account),
        (maker_ta_a, maker_ta_a_account),
        (maker_ta_b, maker_ta_b_account),
        (vault_ta_a, vault_ta_a_account),
        (rent, rent_account),
        (token_program, token_program_account),
        (system_program, system_program_account),
        (event_authority, event_authority_account),
        (program, program_account),
    ];

    mollusk.process_and_validate_instruction(&instruction, accounts.as_ref(), &[]);
}