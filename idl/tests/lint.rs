use quasar_idl::lint::{self, LintRule, Severity};
use quasar_idl::lint::graph::AccountGraph;
use quasar_idl::parser;

fn lint_source(src: &str) -> lint::LintReport {
    let parsed = quasar_idl::parser::parse_program_from_source(src);
    lint::run_lint(&parsed, &lint::LintConfig::default())
}

fn has_diagnostic(report: &lint::LintReport, rule: LintRule, field: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.rule == rule && d.field.as_deref() == Some(field))
}

#[test]
fn lint_report_empty_for_constrained_program() {
    let src = r#"
        declare_id!("11111111111111111111111111111111");

        #[program]
        mod test_program {
            use super::*;

            #[instruction(discriminator = [1])]
            pub fn approve(ctx: Ctx<Approve>) -> Result<(), ProgramError> {
                Ok(())
            }
        }

        #[derive(Accounts)]
        pub struct Approve<'info> {
            pub authority: Signer,
            #[account(mut, has_one = authority)]
            pub vault: Account<Vault<'info>>,
        }

        #[account(discriminator = 1)]
        pub struct Vault {
            pub authority: Address,
            pub balance: u64,
        }
    "#;

    let parsed = quasar_idl::parser::parse_program_from_source(src);
    let report = lint::run_lint(&parsed, &lint::LintConfig::default());
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn parses_has_one_constraints() {
    let src = r#"
        declare_id!("11111111111111111111111111111111");

        #[program]
        mod test_program {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn approve(ctx: Ctx<Approve>) -> Result<(), ProgramError> {
                Ok(())
            }
        }

        #[derive(Accounts)]
        pub struct Approve<'info> {
            pub wallet: Account<Wallet<'info>>,
            pub intent: Account<Intent<'info>>,
            #[account(mut, has_one = wallet, has_one = intent)]
            pub proposal: Account<Proposal<'info>>,
        }

        #[account(discriminator = 1)]
        pub struct Proposal {
            pub wallet: Address,
            pub intent: Address,
        }

        #[account(discriminator = 2)]
        pub struct Wallet {
            pub name: u64,
        }

        #[account(discriminator = 3)]
        pub struct Intent {
            pub threshold: u8,
        }
    "#;

    let parsed = parser::parse_program_from_source(src);
    let proposal_field = parsed.accounts_structs[0]
        .fields
        .iter()
        .find(|f| f.name == "proposal")
        .unwrap();

    assert_eq!(proposal_field.constraints.has_ones, vec!["wallet", "intent"]);
    assert!(proposal_field.constraints.is_mut);
}

#[test]
fn graph_has_correct_edges_for_has_one() {
    let src = r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod test_program {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn approve(ctx: Ctx<Approve>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct Approve<'info> {
            pub authority: Signer,
            pub wallet: Account<Wallet<'info>>,
            #[account(mut, has_one = wallet)]
            pub proposal: Account<Proposal<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Proposal { pub wallet: Address }
        #[account(discriminator = 2)]
        pub struct Wallet { pub bump: u8 }
    "#;
    let parsed = quasar_idl::parser::parse_program_from_source(src);
    let registry = quasar_idl::lint::types::TypeRegistry::from_parsed(&parsed);
    let graph = AccountGraph::build(&parsed.accounts_structs[0], &registry);

    assert_eq!(graph.nodes.len(), 3);
    assert!(graph.has_edge("proposal", "wallet"));
}

// -------------------------------------------------------------------------
// L001 — Island Detection
// -------------------------------------------------------------------------

#[test]
fn l001_island_detection() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            pub vault: Account<Vault<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Vault {
            pub balance: u64,
        }
    "#,
    );
    assert!(
        has_diagnostic(&report, LintRule::L001, "vault"),
        "expected L001 on vault, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn l001_no_false_positive_for_signers() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            #[account(mut, has_one = authority)]
            pub vault: Account<Vault<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Vault {
            pub authority: Address,
        }
    "#,
    );
    assert!(
        report.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn l001_suppressed_by_allow_attribute() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            #[allow(quasar::unconstrained)]
            pub vault: Account<Vault<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Vault {
            pub balance: u64,
        }
    "#,
    );
    assert!(
        !has_diagnostic(&report, LintRule::L001, "vault"),
        "L001 should be suppressed on vault, got: {:?}",
        report.diagnostics
    );
}

// -------------------------------------------------------------------------
// L003 — Missing has_one
// -------------------------------------------------------------------------

#[test]
fn l003_missing_has_one() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub wallet: Account<Wallet<'info>>,
            pub intent: Account<Intent<'info>>,
            #[account(mut, has_one = wallet)]
            pub proposal: Account<Proposal<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Proposal {
            pub wallet: Address,
            pub intent: Address,
        }
        #[account(discriminator = 2)]
        pub struct Wallet { pub bump: u8 }
        #[account(discriminator = 3)]
        pub struct Intent { pub threshold: u8 }
    "#,
    );
    assert!(
        has_diagnostic(&report, LintRule::L003, "proposal"),
        "expected L003 on proposal for missing has_one = intent, got: {:?}",
        report.diagnostics
    );
}

#[test]
fn l003_suppressed_by_pda_seeds() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub wallet: Account<Wallet<'info>>,
            #[account(seeds = [b"proposal", wallet.key()], bump)]
            pub proposal: Account<Proposal<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Proposal {
            pub wallet: Address,
        }
        #[account(discriminator = 2)]
        pub struct Wallet { pub bump: u8 }
    "#,
    );
    assert!(
        !has_diagnostic(&report, LintRule::L003, "proposal"),
        "L003 should not fire when PDA seeds reference the target, got: {:?}",
        report.diagnostics
    );
}

// -------------------------------------------------------------------------
// L007 — Unchecked Account
// -------------------------------------------------------------------------

#[test]
fn l007_unchecked_account() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            pub target: UncheckedAccount,
        }
    "#,
    );
    assert!(
        has_diagnostic(&report, LintRule::L007, "target"),
        "expected L007 on target, got: {:?}",
        report.diagnostics
    );
}

// -------------------------------------------------------------------------
// L002 — Disconnected Subgraph
// -------------------------------------------------------------------------

#[test]
fn l002_disconnected_subgraph() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            #[account(mut, has_one = authority)]
            pub vault: Account<Vault<'info>>,
            pub other_owner: Account<Owner<'info>>,
            #[account(mut, has_one = other_owner)]
            pub ledger: Account<Ledger<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Vault { pub authority: Address }
        #[account(discriminator = 2)]
        pub struct Owner { pub bump: u8 }
        #[account(discriminator = 3)]
        pub struct Ledger { pub other_owner: Address }
    "#,
    );
    let has_l002 = report
        .diagnostics
        .iter()
        .any(|d| d.rule == LintRule::L002);
    assert!(
        has_l002,
        "expected L002 for disconnected subgraphs, got: {:?}",
        report.diagnostics
    );
}

// -------------------------------------------------------------------------
// L004 — Unvalidated Token Mint
// -------------------------------------------------------------------------

#[test]
fn l004_unvalidated_token_mint() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            #[account(mut)]
            pub token_acct: TokenAccount,
            pub mint: Mint,
        }
    "#,
    );
    assert!(
        has_diagnostic(&report, LintRule::L004, "token_acct"),
        "expected L004 on token_acct, got: {:?}",
        report.diagnostics
    );
}

// -------------------------------------------------------------------------
// L005 — Unvalidated Token Authority (writable → Error)
// -------------------------------------------------------------------------

#[test]
fn l005_unvalidated_token_authority_writable_is_error() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            pub authority: Signer,
            #[account(mut, token::mint = mint)]
            pub token_acct: TokenAccount,
            pub mint: Mint,
        }
    "#,
    );
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.rule == LintRule::L005 && d.field.as_deref() == Some("token_acct"));
    assert!(
        diag.is_some(),
        "expected L005 on token_acct, got: {:?}",
        report.diagnostics
    );
    assert_eq!(
        diag.unwrap().severity,
        Severity::Error,
        "writable token account should produce Error severity"
    );
}

// -------------------------------------------------------------------------
// L006 — Writable Without Authority
// -------------------------------------------------------------------------

#[test]
fn l006_writable_without_authority() {
    let report = lint_source(
        r#"
        declare_id!("11111111111111111111111111111111");
        #[program]
        mod p {
            use super::*;
            #[instruction(discriminator = [1])]
            pub fn handler(ctx: Ctx<S>) -> Result<(), ProgramError> { Ok(()) }
        }
        #[derive(Accounts)]
        pub struct S<'info> {
            #[account(mut)]
            pub vault: Account<Vault<'info>>,
        }
        #[account(discriminator = 1)]
        pub struct Vault { pub balance: u64 }
    "#,
    );
    assert!(
        has_diagnostic(&report, LintRule::L006, "vault"),
        "expected L006 on vault, got: {:?}",
        report.diagnostics
    );
}
