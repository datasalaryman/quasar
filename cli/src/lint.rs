use crate::{error::CliResult, LintCommand};

pub fn run(_cmd: LintCommand) -> CliResult {
    // Lint pass removed ��� source-based linting depended on the deleted parser.
    // IDL-based lint (operating on the emitted IDL JSON) will be re-introduced
    // in a future PR.
    println!(
        "  {}",
        crate::style::warn("lint not yet available with idl-build (parser removed)")
    );
    Ok(())
}
