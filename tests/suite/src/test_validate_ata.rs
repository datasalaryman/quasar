use {
    crate::helpers::*,
    quasar_spl::get_associated_token_address_with_program_const,
    quasar_svm::{Account, Pubkey, Instruction},
    quasar_test_token_validate::client::*,
};

// ===========================================================================
// Account<Token> (SPL Token) — ValidateAtaCheck
// ===========================================================================

#[test]
fn ata_spl_happy() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_ok(), "should succeed: {:?}", result.raw_result);
}

#[test]
fn ata_spl_wrong_address() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let wrong_ata = Pubkey::new_unique();

    let instruction: Instruction = ValidateAtaCheckInstruction {
        ata: wrong_ata,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (wrong_ata, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong ATA address should fail");
}

#[test]
fn ata_spl_wrong_mint() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let wrong_mint = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(wrong_mint, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong mint in token data should fail");
}

#[test]
fn ata_spl_wrong_authority() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let wrong_wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wrong_wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong authority in token data should fail");
}

#[test]
fn ata_spl_wrong_owner() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let mut bad_account = token_account(mint_key, wallet, 100, token_program);
    bad_account.owner = Pubkey::default();

    let instruction: Instruction = ValidateAtaCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, bad_account),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong account owner program should fail");
}

// ===========================================================================
// Account<Token2022> — ValidateAta2022Check
// ===========================================================================

#[test]
fn ata_t22_happy() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAta2022CheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_ok(), "should succeed: {:?}", result.raw_result);
}

#[test]
fn ata_t22_wrong_address() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let wrong_ata = Pubkey::new_unique();

    let instruction: Instruction = ValidateAta2022CheckInstruction {
        ata: wrong_ata,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (wrong_ata, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong ATA address should fail");
}

#[test]
fn ata_t22_wrong_mint() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let wrong_mint = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAta2022CheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(wrong_mint, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong mint in token data should fail");
}

#[test]
fn ata_t22_wrong_authority() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let wrong_wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAta2022CheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wrong_wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong authority in token data should fail");
}

#[test]
fn ata_t22_wrong_owner() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let mut bad_account = token_account(mint_key, wallet, 100, token_program);
    bad_account.owner = Pubkey::default();

    let instruction: Instruction = ValidateAta2022CheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, bad_account),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong account owner program should fail");
}

// ===========================================================================
// InterfaceAccount<Token> with SPL Token — ValidateAtaInterfaceCheck
// ===========================================================================

#[test]
fn ata_interface_spl_happy() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_ok(), "should succeed: {:?}", result.raw_result);
}

#[test]
fn ata_interface_spl_wrong_address() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let wrong_ata = Pubkey::new_unique();

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: wrong_ata,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (wrong_ata, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong ATA address should fail");
}

#[test]
fn ata_interface_spl_wrong_mint() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let wrong_mint = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(wrong_mint, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong mint in token data should fail");
}

#[test]
fn ata_interface_spl_wrong_authority() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let wrong_wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wrong_wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong authority in token data should fail");
}

#[test]
fn ata_interface_spl_wrong_owner() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = spl_token_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let mut bad_account = token_account(mint_key, wallet, 100, token_program);
    bad_account.owner = Pubkey::default();

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, bad_account),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong account owner program should fail");
}

// ===========================================================================
// InterfaceAccount<Token> with Token-2022 — ValidateAtaInterfaceCheck
// ===========================================================================

#[test]
fn ata_interface_t22_happy() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_ok(), "should succeed: {:?}", result.raw_result);
}

#[test]
fn ata_interface_t22_wrong_address() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let wrong_ata = Pubkey::new_unique();

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: wrong_ata,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (wrong_ata, token_account(mint_key, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong ATA address should fail");
}

#[test]
fn ata_interface_t22_wrong_mint() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let wrong_mint = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(wrong_mint, wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong mint in token data should fail");
}

#[test]
fn ata_interface_t22_wrong_authority() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let wrong_wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, token_account(mint_key, wrong_wallet, 100, token_program)),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong authority in token data should fail");
}

#[test]
fn ata_interface_t22_wrong_owner() {
    let mut svm = svm_validate();
    let wallet = Pubkey::new_unique();
    let mint_key = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let token_program = token_2022_program_id();
    let (ata_key, _) =
        get_associated_token_address_with_program_const(&wallet, &mint_key, &token_program);

    let mut bad_account = token_account(mint_key, wallet, 100, token_program);
    bad_account.owner = Pubkey::default();

    let instruction: Instruction = ValidateAtaInterfaceCheckInstruction {
        ata: ata_key,
        mint: mint_key,
        wallet,
        token_program,
    }
    .into();

    let result = svm.process_instructions(
        &[instruction],
        &[
            (ata_key, bad_account),
            (mint_key, mint_account(authority, 6, token_program)),
            (wallet, signer_account()),
        ],
    );
    assert!(result.is_err(), "wrong account owner program should fail");
}
