use {
    crate::{config::QuasarConfig, error::CliResult, style, utils},
    std::{
        cmp::Ordering,
        path::PathBuf,
        process::{Command, Stdio},
    },
};

pub fn run(elf_path: Option<PathBuf>, function: Option<String>, source: bool) -> CliResult {
    let so_path = match elf_path {
        Some(p) => p,
        None => find_so()?,
    };

    if !so_path.exists() {
        eprintln!(
            "  {}",
            style::fail(&format!("file not found: {}", so_path.display()))
        );
        std::process::exit(1);
    }

    let objdump = find_objdump().unwrap_or_else(|| {
        eprintln!(
            "  {}",
            style::fail("llvm-objdump not found in Solana platform-tools.")
        );
        eprintln!();
        eprintln!("  Looked in ~/.cache/solana/*/platform-tools/llvm/bin/");
        eprintln!(
            "  Install platform-tools: {}",
            style::bold("solana-install init")
        );
        std::process::exit(1);
    });

    let mut cmd = Command::new(&objdump);
    cmd.arg("-d") // disassemble
        .arg("-C") // demangle
        .arg("--no-show-raw-insn"); // cleaner output

    if source {
        cmd.arg("-S"); // interleave source
    }

    if let Some(ref sym) = function {
        cmd.arg(format!("--disassemble-symbols={sym}"));
    }

    cmd.arg(&so_path);

    // If piping to a pager, let it handle output directly
    let output = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = stdout.lines().collect();

            if lines.is_empty() || (function.is_some() && lines.len() <= 2) {
                if let Some(sym) = function {
                    eprintln!("  {}", style::fail(&format!("symbol not found: {sym}")));
                    eprintln!(
                        "  {}",
                        style::dim("Try a mangled or partial name, e.g. 'entrypoint'")
                    );
                } else {
                    eprintln!("  {}", style::fail("no disassembly output"));
                }
                std::process::exit(1);
            }

            // Print with minimal framing
            for line in &lines {
                println!("{line}");
            }

            // Summary
            let insn_count = lines
                .iter()
                .filter(|l| {
                    let trimmed = l.trim();
                    // Instruction lines start with an address (hex digits followed by colon)
                    trimmed.split(':').next().is_some_and(|addr| {
                        !addr.is_empty() && addr.trim().chars().all(|c| c.is_ascii_hexdigit())
                    })
                })
                .count();

            eprintln!(
                "\n  {} {} instructions ({})",
                style::dim("sBPF"),
                insn_count,
                style::dim(&so_path.display().to_string()),
            );

            Ok(())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("  {}", style::fail("llvm-objdump failed"));
            if !stderr.trim().is_empty() {
                eprintln!("  {}", stderr.trim());
            }
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!(
                "  {}",
                style::fail(&format!("failed to run {}: {e}", objdump.display()))
            );
            std::process::exit(1);
        }
    }
}

fn find_so() -> Result<PathBuf, crate::error::CliError> {
    let config = QuasarConfig::load()?;
    match utils::find_so(&config, true) {
        Some(p) => Ok(p),
        None => {
            eprintln!(
                "  {}",
                style::fail("no .so found in target/deploy/ or target/profile/")
            );
            eprintln!(
                "  {}",
                style::dim("Run `quasar build` first or pass a path: `quasar dump <path>`")
            );
            std::process::exit(1);
        }
    }
}

/// Find llvm-objdump in Solana platform-tools (newest version first)
fn find_objdump() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let cache = home.join(".cache/solana");
    if !cache.exists() {
        return None;
    }

    let mut versions: Vec<_> = std::fs::read_dir(&cache)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            let version = parse_toolchain_version(name)?;
            let objdump = path.join("platform-tools/llvm/bin/llvm-objdump");
            if objdump.exists() {
                Some((version, objdump))
            } else {
                None
            }
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().next().map(|(_, path)| path)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ToolchainVersion(Vec<u64>);

impl Ord for ToolchainVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let max_len = self.0.len().max(other.0.len());
        for idx in 0..max_len {
            let lhs = self.0.get(idx).copied().unwrap_or(0);
            let rhs = other.0.get(idx).copied().unwrap_or(0);
            match lhs.cmp(&rhs) {
                Ordering::Equal => continue,
                non_eq => return non_eq,
            }
        }

        Ordering::Equal
    }
}

impl PartialOrd for ToolchainVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_toolchain_version(name: &str) -> Option<ToolchainVersion> {
    let version = name.strip_prefix('v')?;
    let parts = version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;

    if parts.is_empty() {
        None
    } else {
        Some(ToolchainVersion(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_toolchain_version, ToolchainVersion};

    #[test]
    fn parses_semver_style_versions() {
        assert_eq!(
            parse_toolchain_version("v1.18.22"),
            Some(ToolchainVersion(vec![1, 18, 22]))
        );
        assert_eq!(
            parse_toolchain_version("v2.0"),
            Some(ToolchainVersion(vec![2, 0]))
        );
        assert_eq!(parse_toolchain_version("1.18.22"), None);
        assert_eq!(parse_toolchain_version("v1.18.beta"), None);
    }

    #[test]
    fn compares_versions_numerically() {
        let mut versions = [
            parse_toolchain_version("v1.9.0").expect("parse v1.9.0"),
            parse_toolchain_version("v1.18.22").expect("parse v1.18.22"),
            parse_toolchain_version("v1.10.3").expect("parse v1.10.3"),
            parse_toolchain_version("v2.0.0").expect("parse v2.0.0"),
        ];

        versions.sort();

        assert_eq!(
            versions,
            [
                ToolchainVersion(vec![1, 9, 0]),
                ToolchainVersion(vec![1, 10, 3]),
                ToolchainVersion(vec![1, 18, 22]),
                ToolchainVersion(vec![2, 0, 0]),
            ]
        );
    }
}
