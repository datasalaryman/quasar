use {
    crate::{
        config::{GlobalConfig, GlobalDefaults, UiConfig},
        error::CliResult,
        toolchain,
    },
    dialoguer::{theme::ColorfulTheme, Input, MultiSelect, Select},
    serde::Serialize,
    std::{fmt, fs, path::Path, process::Command},
};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Toolchain {
    Solana,
    Upstream,
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Toolchain::Solana => write!(f, "solana"),
            Toolchain::Upstream => write!(f, "upstream"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TestLanguage {
    None,
    Rust,
    TypeScript,
}

impl fmt::Display for TestLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestLanguage::None => write!(f, "none"),
            TestLanguage::Rust => write!(f, "rust"),
            TestLanguage::TypeScript => write!(f, "typescript"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RustFramework {
    QuasarSVM,
    Mollusk,
}

impl fmt::Display for RustFramework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RustFramework::QuasarSVM => write!(f, "quasar-svm"),
            RustFramework::Mollusk => write!(f, "mollusk"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TypeScriptSdk {
    Kit,
    Web3js,
}

impl fmt::Display for TypeScriptSdk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeScriptSdk::Kit => write!(f, "kit"),
            TypeScriptSdk::Web3js => write!(f, "web3.js"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Template {
    Minimal,
    Full,
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Template::Minimal => write!(f, "minimal"),
            Template::Full => write!(f, "full"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum GitSetup {
    InitializeAndCommit,
    Initialize,
    Skip,
}

impl GitSetup {
    fn from_config(value: Option<&str>) -> Self {
        match value {
            Some("init") => GitSetup::Initialize,
            Some("skip") => GitSetup::Skip,
            _ => GitSetup::InitializeAndCommit,
        }
    }

    fn from_index(idx: usize) -> Self {
        match idx {
            1 => GitSetup::Initialize,
            2 => GitSetup::Skip,
            _ => GitSetup::InitializeAndCommit,
        }
    }

    fn index(self) -> usize {
        match self {
            GitSetup::InitializeAndCommit => 0,
            GitSetup::Initialize => 1,
            GitSetup::Skip => 2,
        }
    }

    fn prompt_label(self) -> &'static str {
        match self {
            GitSetup::InitializeAndCommit => "Initialize + Commit",
            GitSetup::Initialize => "Initialize",
            GitSetup::Skip => "Skip",
        }
    }

    fn summary_label(self) -> &'static str {
        match self {
            GitSetup::InitializeAndCommit => "git: init + commit",
            GitSetup::Initialize => "git: init",
            GitSetup::Skip => "git: skip",
        }
    }
}

impl fmt::Display for GitSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitSetup::InitializeAndCommit => write!(f, "commit"),
            GitSetup::Initialize => write!(f, "init"),
            GitSetup::Skip => write!(f, "skip"),
        }
    }
}

#[derive(Debug, Clone)]
enum PackageManager {
    Pnpm,
    Bun,
    Npm,
    Yarn,
    Other { install: String, test: String },
}

impl PackageManager {
    fn install_cmd(&self) -> &str {
        match self {
            PackageManager::Pnpm => "pnpm install",
            PackageManager::Bun => "bun install",
            PackageManager::Npm => "npm install",
            PackageManager::Yarn => "yarn install",
            PackageManager::Other { install, .. } => install,
        }
    }

    fn test_cmd(&self) -> &str {
        match self {
            PackageManager::Pnpm => "pnpm test",
            PackageManager::Bun => "bun test",
            PackageManager::Npm => "npm test",
            PackageManager::Yarn => "yarn test",
            PackageManager::Other { test, .. } => test,
        }
    }

    fn from_config(value: Option<&str>) -> usize {
        match value {
            Some("bun") => 1,
            Some("npm") => 2,
            Some("yarn") => 3,
            _ => 0, // pnpm default
        }
    }
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageManager::Pnpm => write!(f, "pnpm"),
            PackageManager::Bun => write!(f, "bun"),
            PackageManager::Npm => write!(f, "npm"),
            PackageManager::Yarn => write!(f, "yarn"),
            PackageManager::Other { .. } => write!(f, "other"),
        }
    }
}

// ---------------------------------------------------------------------------
// Quasar.toml schema
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct QuasarToml {
    project: QuasarProject,
    toolchain: QuasarToolchain,
    testing: QuasarTesting,
    clients: QuasarClients,
}

#[derive(Serialize)]
struct QuasarProject {
    name: String,
}

#[derive(Serialize)]
struct QuasarToolchain {
    #[serde(rename = "type")]
    toolchain_type: String,
}

#[derive(Serialize)]
struct QuasarTesting {
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust: Option<QuasarRustTesting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    typescript: Option<QuasarTypeScriptTesting>,
}

#[derive(Serialize)]
struct QuasarRustTesting {
    framework: String,
    test: String,
}

#[derive(Serialize)]
struct QuasarTypeScriptTesting {
    framework: String,
    sdk: String,
    install: String,
    test: String,
}

#[derive(Serialize)]
struct QuasarClients {
    languages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Banner — sparse blue aurora + FIGlet "Quasar" text reveal
// ---------------------------------------------------------------------------

fn print_banner() {
    use std::io::{self, IsTerminal, Write};

    let stdout = io::stdout();
    if !stdout.is_terminal() {
        println!("\n  Quasar\n  Build programs that execute at the speed of light\n");
        return;
    }

    use std::{thread, time::Duration};

    // Restore cursor if interrupted during animation
    ctrlc::set_handler(move || {
        print!("\x1b[?25h");
        std::process::exit(130);
    })
    .ok();

    let mut out = stdout.lock();
    write!(out, "\x1b[?25l").ok();

    let w: usize = 70;
    let h: usize = 11; // 1 blank + 7 figlet + 1 blank + 1 tagline + 1 byline
    let n_frames: usize = 22;
    let nebula_w: f32 = 30.0; // width of the sweeping nebula band

    // FIGlet "Quasar" — block style, 7 lines tall
    #[rustfmt::skip]
    let figlet: [&str; 7] = [
        " ██████╗ ██╗   ██╗ █████╗ ███████╗ █████╗ ██████╗ ",
        "██╔═══██╗██║   ██║██╔══██╗██╔════╝██╔══██╗██╔══██╗",
        "██║   ██║██║   ██║███████║███████╗███████║██████╔╝",
        "██║▄▄ ██║██║   ██║██╔══██║╚════██║██╔══██║██╔══██╗",
        "╚██████╔╝╚██████╔╝██║  ██║███████║██║  ██║██║  ██║",
        " ╚══▀▀═╝  ╚═════╝ ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝",
        "",
    ];
    let fig: Vec<Vec<char>> = figlet.iter().map(|l| l.chars().collect()).collect();
    let fig_w = fig.iter().map(|l| l.len()).max().unwrap_or(0);
    let fig_off = w.saturating_sub(fig_w) / 2;

    let tagline = "Build programs that execute at the speed of light";
    let tag_chars: Vec<char> = tagline.chars().collect();
    let tag_off = w.saturating_sub(tag_chars.len()) / 2;

    let byline = "by blueshift.gg";
    let by_chars: Vec<char> = byline.chars().collect();
    let by_off = w.saturating_sub(by_chars.len()) / 2;

    // Reserve space
    writeln!(out).ok();
    for _ in 0..h {
        writeln!(out).ok();
    }
    out.flush().ok();

    for frame in 0..n_frames {
        write!(out, "\x1b[{h}A").ok();
        let is_final = frame == n_frames - 1;

        // Leading edge sweeps left → right, revealing text in its wake
        let t = frame as f32 / (n_frames - 2).max(1) as f32;
        let edge = -nebula_w + t * (w as f32 + nebula_w * 2.0);

        #[allow(clippy::needless_range_loop)]
        for li in 0..h {
            write!(out, "\x1b[2K  ").ok();

            if is_final {
                // ── Final clean frame ──
                match li {
                    1..=7 => {
                        let row = &fig[li - 1];
                        for _ in 0..fig_off {
                            write!(out, " ").ok();
                        }
                        for &ch in row.iter() {
                            if ch != ' ' {
                                write!(out, "\x1b[36m{ch}\x1b[0m").ok();
                            } else {
                                write!(out, " ").ok();
                            }
                        }
                    }
                    9 => {
                        for _ in 0..tag_off {
                            write!(out, " ").ok();
                        }
                        write!(out, "\x1b[1m{tagline}\x1b[0m").ok();
                    }
                    10 => {
                        for _ in 0..by_off {
                            write!(out, " ").ok();
                        }
                        write!(out, "\x1b[90mby \x1b[36mblueshift.gg\x1b[0m").ok();
                    }
                    _ => {}
                }
            } else {
                // ── Nebula sweep: reveals text as it passes ──
                for ci in 0..w {
                    let dist = ci as f32 - edge;

                    // Text character at this position
                    let text_ch = match li {
                        1..=7 if ci >= fig_off && ci - fig_off < fig_w => {
                            fig[li - 1].get(ci - fig_off).copied().unwrap_or(' ')
                        }
                        9 if ci >= tag_off && ci - tag_off < tag_chars.len() => {
                            tag_chars[ci - tag_off]
                        }
                        10 if ci >= by_off && ci - by_off < by_chars.len() => by_chars[ci - by_off],
                        _ => ' ',
                    };

                    if dist < -nebula_w {
                        // Behind the nebula: text fully revealed
                        write_text_char(&mut out, text_ch, li, ci, by_off);
                    } else if dist < nebula_w {
                        // Inside the nebula band
                        let blend = (dist + nebula_w) / (nebula_w * 2.0);
                        let intensity = 1.0 - (dist.abs() / nebula_w);
                        let d = aurora_density(ci, li, frame) * intensity;

                        if blend < 0.3 && text_ch != ' ' {
                            // Trailing edge: text bleeds through
                            write_text_char(&mut out, text_ch, li, ci, by_off);
                        } else {
                            write_nebula_char(&mut out, d);
                        }
                    } else {
                        // Ahead of nebula: dark
                        write!(out, " ").ok();
                    }
                }
            }
            writeln!(out).ok();
        }
        out.flush().ok();

        if !is_final {
            thread::sleep(Duration::from_millis(55));
        }
    }

    write!(out, "\x1b[?25h").ok();
    writeln!(out).ok();
    out.flush().ok();
}

fn write_text_char(
    out: &mut impl std::io::Write,
    ch: char,
    line: usize,
    col: usize,
    by_off: usize,
) {
    if ch == ' ' {
        write!(out, " ").ok();
    } else {
        match line {
            1..=7 => {
                write!(out, "\x1b[36m{ch}\x1b[0m").ok();
            }
            9 => {
                write!(out, "\x1b[1m{ch}\x1b[0m").ok();
            }
            10 => {
                if col - by_off < 3 {
                    write!(out, "\x1b[90m{ch}\x1b[0m").ok();
                } else {
                    write!(out, "\x1b[36m{ch}\x1b[0m").ok();
                }
            }
            _ => {
                write!(out, " ").ok();
            }
        };
    }
}

fn write_nebula_char(out: &mut impl std::io::Write, d: f32) {
    if d < 0.10 {
        write!(out, " ").ok();
    } else if d < 0.25 {
        write!(out, "\x1b[38;2;15;25;85m░\x1b[0m").ok();
    } else if d < 0.42 {
        write!(out, "\x1b[38;2;30;55;145m░\x1b[0m").ok();
    } else if d < 0.60 {
        write!(out, "\x1b[38;2;50;95;200m▒\x1b[0m").ok();
    } else if d < 0.78 {
        write!(out, "\x1b[38;2;75;140;235m▓\x1b[0m").ok();
    } else {
        write!(out, "\x1b[38;2;100;170;255m█\x1b[0m").ok();
    }
}

/// Aurora density — sine waves flowing rightward, tuned for sparse output.
fn aurora_density(col: usize, line: usize, frame: usize) -> f32 {
    let c = col as f32;
    let l = line as f32;
    let f = frame as f32;

    let w1 = ((c - f * 5.0) / 8.0 + l * 0.35).sin();
    let w2 = ((c - f * 3.5) / 5.5 - l * 0.25).sin() * 0.45;
    let w3 = ((c - f * 7.0) / 12.0 + l * 0.15).sin() * 0.3;

    ((w1 + w2 + w3 + 1.5) / 3.5).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// ANSI helpers (delegate to shared style module)
// ---------------------------------------------------------------------------

fn color(code: u8, s: &str) -> String {
    crate::style::color(code, s)
}

fn bold(s: &str) -> String {
    crate::style::bold(s)
}

fn dim(s: &str) -> String {
    crate::style::dim(s)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(
    name: Option<String>,
    yes: bool,
    no_git: bool,
    test_language_override: Option<String>,
    rust_framework_override: Option<String>,
    ts_sdk_override: Option<String>,
    template_override: Option<String>,
    toolchain_override: Option<String>,
) -> CliResult {
    let globals = GlobalConfig::load();

    // Skip prompts when a name is provided (or --yes is set), or when explicit
    // flags given
    let skip_prompts = yes
        || name.is_some()
        || test_language_override.is_some()
        || rust_framework_override.is_some()
        || ts_sdk_override.is_some()
        || template_override.is_some()
        || toolchain_override.is_some();

    // Validate explicit flag values before proceeding
    if let Some(ref t) = test_language_override {
        if !matches!(t.as_str(), "none" | "rust" | "typescript") {
            eprintln!(
                "  {}",
                crate::style::fail(&format!("unknown test language: {t}"))
            );
            eprintln!("  {}", dim("valid: none, rust, typescript"));
            std::process::exit(1);
        }
    }
    if let Some(ref f) = rust_framework_override {
        if !matches!(f.as_str(), "quasar-svm" | "mollusk") {
            eprintln!(
                "  {}",
                crate::style::fail(&format!("unknown rust framework: {f}"))
            );
            eprintln!("  {}", dim("valid: quasar-svm, mollusk"));
            std::process::exit(1);
        }
    }
    if let Some(ref s) = ts_sdk_override {
        if !matches!(s.as_str(), "kit" | "web3.js") {
            eprintln!(
                "  {}",
                crate::style::fail(&format!("unknown TypeScript SDK: {s}"))
            );
            eprintln!("  {}", dim("valid: kit, web3.js"));
            std::process::exit(1);
        }
    }
    if let Some(ref t) = template_override {
        if !matches!(t.as_str(), "minimal" | "full") {
            eprintln!(
                "  {}",
                crate::style::fail(&format!("unknown template: {t}"))
            );
            eprintln!("  {}", dim("valid: minimal, full"));
            std::process::exit(1);
        }
    }
    if let Some(ref t) = toolchain_override {
        if !matches!(t.as_str(), "solana" | "upstream") {
            eprintln!(
                "  {}",
                crate::style::fail(&format!("unknown toolchain: {t}"))
            );
            eprintln!("  {}", dim("valid: solana, upstream"));
            std::process::exit(1);
        }
    }

    if globals.ui.animation && !skip_prompts {
        print_banner();
    }

    let theme = ColorfulTheme::default();

    // Project name
    let name: String = if skip_prompts {
        name.unwrap_or_else(|| {
            eprintln!(
                "  {}",
                crate::style::fail("a project name is required when using flags")
            );
            eprintln!(
                "  {}",
                crate::style::dim("usage: quasar init <name> [--test-language ...] [--template ...]")
            );
            std::process::exit(1);
        })
    } else {
        let mut prompt = Input::with_theme(&theme).with_prompt("Project name");
        if let Some(default) = name {
            prompt = prompt.default(default);
        }
        prompt.interact_text().map_err(anyhow::Error::from)?
    };

    // When scaffolding into ".", derive the crate name from the current directory
    let crate_name = if name == "." {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "my-program".to_string())
    } else {
        name.clone()
    };

    // Toolchain
    let toolchain_default = match toolchain_override
        .as_deref()
        .or(globals.defaults.toolchain.as_deref())
    {
        Some("upstream") => 1,
        _ => 0,
    };
    let toolchain_idx = if skip_prompts {
        toolchain_default
    } else {
        let toolchain_items = &[
            "solana    (cargo build-sbf)",
            "upstream  (cargo +nightly build-bpf)",
        ];
        Select::with_theme(&theme)
            .with_prompt("Toolchain")
            .items(toolchain_items)
            .default(toolchain_default)
            .interact()
            .map_err(anyhow::Error::from)?
    };
    let toolchain = match toolchain_idx {
        0 => Toolchain::Solana,
        _ => Toolchain::Upstream,
    };

    // For upstream: sbpf-linker must be installed
    if matches!(toolchain, Toolchain::Upstream) && !toolchain::has_sbpf_linker() {
        eprintln!();
        eprintln!("  {} sbpf-linker not found.", color(196, "\u{2718}"));
        eprintln!();
        eprintln!("  Install platform-tools first:");
        eprintln!(
            "    {}",
            bold("git clone https://github.com/anza-xyz/platform-tools")
        );
        eprintln!("    {}", bold("cd platform-tools"));
        eprintln!("    {}", bold("cargo install-with-gallery"));
        eprintln!();
        std::process::exit(1);
    }

    let lang_default = match test_language_override
        .as_deref()
        .or(globals.defaults.test_language.as_deref())
    {
        Some("none") => 0,
        Some("typescript") => 2,
        _ => 1, // rust default
    };
    let rust_fw_default = match rust_framework_override
        .as_deref()
        .or(globals.defaults.rust_framework.as_deref())
    {
        Some("mollusk") => 1,
        _ => 0, // quasar-svm default
    };
    let ts_sdk_default = match ts_sdk_override
        .as_deref()
        .or(globals.defaults.ts_sdk.as_deref())
    {
        Some("web3.js") => 1,
        _ => 0, // kit default
    };

    // Test language
    let test_lang_idx = if skip_prompts {
        lang_default
    } else {
        let lang_items = &["None", "Rust", "TypeScript"];
        Select::with_theme(&theme)
            .with_prompt("Test language")
            .items(lang_items)
            .default(lang_default)
            .interact()
            .map_err(anyhow::Error::from)?
    };
    let test_language = match test_lang_idx {
        1 => TestLanguage::Rust,
        2 => TestLanguage::TypeScript,
        _ => TestLanguage::None,
    };

    // Rust test framework (only if Rust)
    let rust_framework = if matches!(test_language, TestLanguage::Rust) {
        let idx = if skip_prompts {
            rust_fw_default
        } else {
            let items = &["QuasarSVM", "Mollusk"];
            Select::with_theme(&theme)
                .with_prompt("Rust test framework")
                .items(items)
                .default(rust_fw_default)
                .interact()
                .map_err(anyhow::Error::from)?
        };
        Some(match idx {
            1 => RustFramework::Mollusk,
            _ => RustFramework::QuasarSVM,
        })
    } else {
        None
    };

    // TypeScript SDK (only if TypeScript)
    let ts_sdk = if matches!(test_language, TestLanguage::TypeScript) {
        let idx = if skip_prompts {
            ts_sdk_default
        } else {
            let items = &["Kit", "Web3.js"];
            Select::with_theme(&theme)
                .with_prompt("TypeScript SDK")
                .items(items)
                .default(ts_sdk_default)
                .interact()
                .map_err(anyhow::Error::from)?
        };
        Some(match idx {
            1 => TypeScriptSdk::Web3js,
            _ => TypeScriptSdk::Kit,
        })
    } else {
        None
    };

    // Package manager (only for TypeScript)
    let package_manager = if matches!(test_language, TestLanguage::TypeScript) {
        let pm_default = PackageManager::from_config(globals.defaults.package_manager.as_deref());
        let pm_idx = if skip_prompts {
            pm_default
        } else {
            let pm_items = &["pnpm", "bun", "npm", "yarn", "other"];
            Select::with_theme(&theme)
                .with_prompt("Package manager")
                .items(pm_items)
                .default(pm_default)
                .interact()
                .map_err(anyhow::Error::from)?
        };
        Some(match pm_idx {
            0 => PackageManager::Pnpm,
            1 => PackageManager::Bun,
            2 => PackageManager::Npm,
            3 => PackageManager::Yarn,
            _ => {
                let install: String = Input::with_theme(&theme)
                    .with_prompt("Install command")
                    .default("pnpm install".into())
                    .interact_text()
                    .map_err(anyhow::Error::from)?;
                let test: String = Input::with_theme(&theme)
                    .with_prompt("Test command")
                    .default("pnpm test".into())
                    .interact_text()
                    .map_err(anyhow::Error::from)?;
                PackageManager::Other { install, test }
            }
        })
    } else {
        None
    };

    // Client languages — Rust always included, test language forced on
    let ts_tests = matches!(test_language, TestLanguage::TypeScript);
    let client_languages: Vec<String> = if skip_prompts {
        let mut langs = vec!["rust".to_string()];
        if ts_tests {
            langs.push("typescript".to_string());
        }
        langs
    } else {
        // Forced languages shown in prompt text, not selectable
        let mut forced = vec!["Rust"];
        if ts_tests {
            forced.push("TypeScript");
        }

        let all_optional: &[(&str, &str)] = &[
            ("TypeScript", "typescript"),
            ("Golang (Experimental)", "golang"),
            ("Python (Experimental)", "python"),
        ];
        let optional: Vec<(&str, &str)> = all_optional
            .iter()
            .copied()
            .filter(|(display, _)| !forced.contains(display))
            .collect();

        let prompt = format!(
            "Additional client languages ({} always included)",
            forced.join(", ")
        );

        let display_items: Vec<&str> = optional.iter().map(|(d, _)| *d).collect();
        let selected = MultiSelect::with_theme(&theme)
            .with_prompt(&prompt)
            .items(&display_items)
            .interact()
            .map_err(anyhow::Error::from)?;

        let mut langs: Vec<String> = vec!["rust".to_string()];
        if ts_tests {
            langs.push("typescript".to_string());
        }
        for &i in &selected {
            langs.push(optional[i].1.to_string());
        }
        langs
    };

    // Template
    let template_default = match template_override
        .as_deref()
        .or(globals.defaults.template.as_deref())
    {
        Some("full") => 1,
        _ => 0,
    };
    let template_idx = if skip_prompts {
        template_default
    } else {
        let template_items = &[
            "Minimal (instruction file only)",
            "Full (state, errors, and instruction files)",
        ];
        Select::with_theme(&theme)
            .with_prompt("Template")
            .items(template_items)
            .default(template_default)
            .interact()
            .map_err(anyhow::Error::from)?
    };
    let template = match template_idx {
        0 => Template::Minimal,
        _ => Template::Full,
    };

    // Git setup
    let git_default = GitSetup::from_config(globals.defaults.git.as_deref());
    let git_setup = if no_git {
        GitSetup::Skip
    } else if skip_prompts {
        git_default
    } else {
        let git_items = &[
            GitSetup::InitializeAndCommit.prompt_label(),
            GitSetup::Initialize.prompt_label(),
            GitSetup::Skip.prompt_label(),
        ];
        let git_idx = Select::with_theme(&theme)
            .with_prompt("Initialize a new git repo?")
            .items(git_items)
            .default(git_default.index())
            .interact()
            .map_err(anyhow::Error::from)?;
        GitSetup::from_index(git_idx)
    };

    if skip_prompts {
        println!();
        let fw_label = match test_language {
            TestLanguage::None => "no tests".to_string(),
            TestLanguage::Rust => format!("rust/{}", rust_framework.unwrap()),
            TestLanguage::TypeScript => format!("typescript/{}", ts_sdk.unwrap()),
        };
        println!(
            "  {} {} {} {} {} {} {}",
            dim("Using:"),
            bold(&toolchain.to_string()),
            dim("+"),
            bold(&fw_label),
            bold(&template.to_string()),
            dim("+"),
            bold(git_setup.summary_label()),
        );
    }

    scaffold(&name, &crate_name, toolchain, test_language, rust_framework, ts_sdk, template, package_manager.as_ref(), &client_languages)?;

    // Optional git setup (unless already in a git repo)
    maybe_initialize_git_repo(&name, git_setup);

    // Save preferences for next time (disable animation after first run)
    let saved_git_default = if no_git {
        globals.defaults.git.clone()
    } else {
        Some(git_setup.to_string())
    };
    let saved_pm = package_manager
        .as_ref()
        .map(|pm| pm.to_string())
        .or_else(|| globals.defaults.package_manager.clone());

    let new_globals = GlobalConfig {
        defaults: GlobalDefaults {
            toolchain: Some(toolchain.to_string()),
            test_language: Some(test_language.to_string()),
            rust_framework: rust_framework.map(|f| f.to_string()),
            ts_sdk: ts_sdk.map(|s| s.to_string()),
            template: Some(template.to_string()),
            git: saved_git_default,
            package_manager: saved_pm,
        },
        ui: UiConfig {
            animation: false,
            ..globals.ui
        },
    };
    let _ = new_globals.save(); // best-effort

    // Success message
    println!();
    println!(
        "  {}  Created {} {}",
        color(83, "\u{2714}"),
        bold(&crate_name),
        dim("project")
    );
    println!();
    println!("  {}", dim("Next steps:"));
    if name != "." {
        println!(
            "    {}  {}",
            color(45, "\u{276f}"),
            bold(&format!("cd {name}"))
        );
    }
    println!("    {}  {}", color(45, "\u{276f}"), bold("quasar build"));
    if !matches!(test_language, TestLanguage::None) {
        println!("    {}  {}", color(45, "\u{276f}"), bold("quasar test"));
    }
    println!();
    println!(
        "  {} saved to {}",
        dim("Preferences"),
        dim(&GlobalConfig::path().display().to_string()),
    );
    println!();

    Ok(())
}

fn maybe_initialize_git_repo(name: &str, git_setup: GitSetup) {
    if matches!(git_setup, GitSetup::Skip) {
        return;
    }

    let root = Path::new(name);
    let already_git = if name == "." {
        Path::new(".git").exists()
    } else {
        root.join(".git").exists()
    };

    if !already_git {
        let _ = initialize_git_repo(root, git_setup);
    }
}

fn initialize_git_repo(root: &Path, git_setup: GitSetup) -> bool {
    run_git(root, &["init", "--quiet"])
        && match git_setup {
            GitSetup::InitializeAndCommit => {
                run_git(root, &["add", "."])
                    && run_git(root, &["commit", "-am", "chore: initial commit", "--quiet"])
            }
            GitSetup::Initialize | GitSetup::Skip => true,
        }
}

fn run_git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn scaffold(
    dir: &str,
    name: &str,
    toolchain: Toolchain,
    test_language: TestLanguage,
    rust_framework: Option<RustFramework>,
    ts_sdk: Option<TypeScriptSdk>,
    template: Template,
    package_manager: Option<&PackageManager>,
    client_languages: &[String],
) -> CliResult {
    let root = Path::new(dir);

    if dir == "." {
        // Scaffold into current directory — check it doesn't already have a project
        if root.join("Cargo.toml").exists() || root.join("Quasar.toml").exists() {
            eprintln!(
                "  {}",
                crate::style::fail("current directory already contains a project")
            );
            std::process::exit(1);
        }
    } else if root.exists() {
        eprintln!(
            "  {}",
            crate::style::fail(&format!("directory '{dir}' already exists"))
        );
        std::process::exit(1);
    }

    let src = root.join("src");
    fs::create_dir_all(&src).map_err(anyhow::Error::from)?;

    // Quasar.toml
    let config = QuasarToml {
        project: QuasarProject {
            name: name.to_string(),
        },
        toolchain: QuasarToolchain {
            toolchain_type: toolchain.to_string(),
        },
        testing: QuasarTesting {
            language: test_language.to_string(),
            rust: match (test_language, rust_framework) {
                (TestLanguage::Rust, Some(fw)) => Some(QuasarRustTesting {
                    framework: fw.to_string(),
                    test: "cargo test tests::".to_string(),
                }),
                _ => None,
            },
            typescript: match (test_language, ts_sdk) {
                (TestLanguage::TypeScript, Some(sdk)) => {
                    let pm = package_manager.expect("package_manager required for TS");
                    Some(QuasarTypeScriptTesting {
                        framework: "quasar-svm".to_string(),
                        sdk: sdk.to_string(),
                        install: pm.install_cmd().to_string(),
                        test: pm.test_cmd().to_string(),
                    })
                }
                _ => None,
            },
        },
        clients: QuasarClients {
            languages: client_languages.to_vec(),
        },
    };
    let toml_str = toml::to_string_pretty(&config).map_err(anyhow::Error::from)?;
    fs::write(root.join("Quasar.toml"), toml_str).map_err(anyhow::Error::from)?;

    // Cargo.toml
    fs::write(
        root.join("Cargo.toml"),
        generate_cargo_toml(name, toolchain, test_language, rust_framework),
    )
    .map_err(anyhow::Error::from)?;

    // .cargo/config.toml (upstream only)
    if matches!(toolchain, Toolchain::Upstream) {
        let cargo_dir = root.join(".cargo");
        fs::create_dir_all(&cargo_dir).map_err(anyhow::Error::from)?;
        fs::write(cargo_dir.join("config.toml"), CARGO_CONFIG).map_err(anyhow::Error::from)?;
    }

    // .gitignore
    fs::write(root.join(".gitignore"), GITIGNORE).map_err(anyhow::Error::from)?;

    // Generate program keypair
    let deploy_dir = root.join("target").join("deploy");
    fs::create_dir_all(&deploy_dir).map_err(anyhow::Error::from)?;

    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    let program_id = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();

    // Write keypair as Solana CLI-compatible JSON (64-byte array: secret + public)
    let mut keypair_bytes = Vec::with_capacity(64);
    keypair_bytes.extend_from_slice(signing_key.as_bytes());
    keypair_bytes.extend_from_slice(signing_key.verifying_key().as_bytes());
    let keypair_json = serde_json::to_string(&keypair_bytes).map_err(anyhow::Error::from)?;
    fs::write(
        deploy_dir.join(format!("{name}-keypair.json")),
        &keypair_json,
    )
    .map_err(anyhow::Error::from)?;

    // src/lib.rs
    let module_name = name.replace('-', "_");
    let has_rust_tests = matches!(test_language, TestLanguage::Rust);
    fs::write(
        src.join("lib.rs"),
        generate_lib_rs(&module_name, &program_id, template, has_rust_tests),
    )
    .map_err(anyhow::Error::from)?;

    // Template-specific files
    match template {
        Template::Minimal => {
            // Everything lives in lib.rs — no instructions/ directory needed
        }
        Template::Full => {
            let instructions_dir = src.join("instructions");
            fs::create_dir_all(&instructions_dir).map_err(anyhow::Error::from)?;
            fs::write(instructions_dir.join("mod.rs"), INSTRUCTIONS_MOD)
                .map_err(anyhow::Error::from)?;
            fs::write(
                instructions_dir.join("initialize.rs"),
                INSTRUCTION_INITIALIZE,
            )
            .map_err(anyhow::Error::from)?;
            fs::write(src.join("state.rs"), STATE_RS).map_err(anyhow::Error::from)?;
            fs::write(src.join("errors.rs"), ERRORS_RS).map_err(anyhow::Error::from)?;
        }
    }

    // Rust test scaffold
    if let Some(fw) = rust_framework {
        fs::write(
            src.join("tests.rs"),
            generate_tests_rs(&module_name, fw, template, toolchain),
        )
        .map_err(anyhow::Error::from)?;
    }

    // TypeScript test scaffold
    if let Some(sdk) = ts_sdk {
        let tests_dir = root.join("tests");
        fs::create_dir_all(&tests_dir).map_err(anyhow::Error::from)?;

        fs::write(
            root.join("package.json"),
            generate_package_json(name, sdk),
        )
        .map_err(anyhow::Error::from)?;
        fs::write(root.join("tsconfig.json"), TS_TEST_TSCONFIG).map_err(anyhow::Error::from)?;

        fs::write(
            tests_dir.join(format!("{}.test.ts", name)),
            generate_test_ts(name, sdk, toolchain),
        )
        .map_err(anyhow::Error::from)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn generate_cargo_toml(name: &str, toolchain: Toolchain, test_language: TestLanguage, rust_framework: Option<RustFramework>) -> String {
    let mut out = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lints.rust.unexpected_cfgs]
level = "warn"
check-cfg = [
    'cfg(target_os, values("solana"))',
]

[lib]
crate-type = ["cdylib"]

[features]
alloc = []
client = []
debug = []

[dependencies]
quasar-lang = "0.0"
"#,
    );

    if matches!(toolchain, Toolchain::Solana) {
        out.push_str("solana-instruction = { version = \"3.2.0\" }\n");
    }

    // Dev dependencies based on testing framework
    let client_dep = format!("{name}-client = {{ path = \"target/client/rust/{name}-client\" }}\n");

    match (test_language, rust_framework) {
        (TestLanguage::None, _) => {}
        (TestLanguage::Rust, Some(RustFramework::Mollusk)) => {
            out.push_str(&format!(
                r#"
[dev-dependencies]
{client_dep}mollusk-svm = "0.10.3"
solana-account = {{ version = "3.4.0" }}
solana-address = {{ version = "2.2.0", features = ["decode"] }}
solana-instruction = {{ version = "3.2.0", features = ["bincode"] }}
"#,
            ));
        }
        (TestLanguage::Rust, _) => {
            out.push_str(&format!(
                r#"
[dev-dependencies]
{client_dep}quasar-svm = {{ version = "0.1" }}
solana-account = {{ version = "3.4.0" }}
solana-address = {{ version = "2.2.0", features = ["decode"] }}
solana-instruction = {{ version = "3.2.0", features = ["bincode"] }}
solana-pubkey = {{ version = "4.1.0" }}
"#,
            ));
        }
        (TestLanguage::TypeScript, _) => {
            out.push_str(&format!(
                r#"
[dev-dependencies]
{client_dep}solana-account = {{ version = "3.4.0" }}
solana-address = {{ version = "2.2.0", features = ["decode"] }}
solana-instruction = {{ version = "3.2.0", features = ["bincode"] }}
"#,
            ));
        }
    }

    out
}

fn generate_lib_rs(
    module_name: &str,
    program_id: &str,
    template: Template,
    has_tests: bool,
) -> String {
    let test_mod = if has_tests {
        "\n#[cfg(test)]\nmod tests;\n"
    } else {
        ""
    };

    match template {
        Template::Minimal => {
            format!(
                r#"#![cfg_attr(not(test), no_std)]

use quasar_lang::prelude::*;

declare_id!("{program_id}");

#[derive(Accounts)]
pub struct Initialize<'info> {{
    pub payer: &'info mut Signer,
    pub system_program: &'info Program<System>,
}}

impl<'info> Initialize<'info> {{
    #[inline(always)]
    pub fn initialize(&self) -> Result<(), ProgramError> {{
        Ok(())
    }}
}}

#[program]
mod {module_name} {{
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {{
        ctx.accounts.initialize()
    }}
}}
{test_mod}"#
            )
        }
        Template::Full => {
            format!(
                r#"#![cfg_attr(not(test), no_std)]

use quasar_lang::prelude::*;

mod errors;
mod instructions;
mod state;
use instructions::*;

declare_id!("{program_id}");

#[program]
mod {module_name} {{
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {{
        ctx.accounts.initialize()
    }}
}}
{test_mod}"#
            )
        }
    }
}

fn generate_package_json(name: &str, ts_sdk: TypeScriptSdk) -> String {
    let solana_dep = if matches!(ts_sdk, TypeScriptSdk::Kit) {
        "\"@solana/kit\": \"^6.0.0\""
    } else {
        "\"@solana/web3.js\": \"github:blueshift-gg/solana-web3.js#v2\""
    };
    format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "private": true,
  "scripts": {{
    "test": "vitest run"
  }},
  "dependencies": {{
    "@blueshift-gg/quasar-svm": "^0.1",
    {solana_dep}
  }},
  "devDependencies": {{
    "@types/node": "^22.0.0",
    "typescript": "^5.9.3",
    "vitest": "^3.1.0"
  }}
}}
"#
    )
}

fn generate_test_ts(name: &str, ts_sdk: TypeScriptSdk, toolchain: Toolchain) -> String {
    let module_name = name.replace('-', "_");
    let class_name = crate::utils::snake_to_pascal(&module_name);
    let so_name = match toolchain {
        Toolchain::Upstream => format!("lib{module_name}"),
        Toolchain::Solana => module_name.clone(),
    };

    if matches!(ts_sdk, TypeScriptSdk::Kit) {
        format!(
            r#"import {{ generateKeyPairSigner }} from "@solana/kit";
import {{ {class_name}Client, PROGRAM_ADDRESS }} from "../target/client/typescript/{module_name}/kit";
import {{ describe, it, expect }} from "vitest";
import {{ QuasarSvm, createKeyedSystemAccount }} from "@blueshift-gg/quasar-svm/kit";
import {{ readFile }} from "node:fs/promises";

const {class_name}Program = new {class_name}Client();

describe.concurrent("{class_name} Program", async () => {{
  const vm = new QuasarSvm();
  vm.addProgram(PROGRAM_ADDRESS, await readFile("target/deploy/{so_name}.so"));

  it("initializes", async () => {{
    const payer = await generateKeyPairSigner();

    const initializeInstruction = {class_name}Program.createInitializeInstruction({{
      payer: payer.address,
    }});

    const result = vm.processInstruction(initializeInstruction, [
      createKeyedSystemAccount(payer.address),
    ]);

    expect(result.status.ok, `initialize failed:\n${{result.logs.join("\n")}}`).toBe(true);
  }});
}});
"#
        )
    } else {
        format!(
            r#"import {{ Keypair }} from "@solana/web3.js";
import {{ {class_name}Client }} from "../target/client/typescript/{module_name}/web3.js";
import {{ readFile }} from "node:fs/promises";
import {{ describe, it, expect }} from "vitest";
import {{ QuasarSvm, createKeyedSystemAccount }} from "@blueshift-gg/quasar-svm/web3.js";

const {class_name}Program = new {class_name}Client();

describe.concurrent("{class_name} Program", async () => {{
  const vm = new QuasarSvm();
  vm.addProgram({class_name}Client.programId, await readFile("target/deploy/{so_name}.so"));

  it("initializes", async () => {{
    const {{ publicKey: payer }} = await Keypair.generate();

    const initializeInstruction = {class_name}Program.createInitializeInstruction({{
      payer,
    }});

    const result = vm.processInstruction(initializeInstruction, [
      createKeyedSystemAccount(payer),
    ]);

    expect(result.status.ok, `initialize failed:\n${{result.logs.join("\n")}}`).toBe(true);
  }});
}});
"#
        )
    }
}

fn generate_tests_rs(
    module_name: &str,
    rust_framework: RustFramework,
    template: Template,
    toolchain: Toolchain,
) -> String {
    let mut libname = module_name.to_string();
    if matches!(toolchain, Toolchain::Upstream) {
        libname = format!("lib{libname}");
    };
    let client_crate = format!("{module_name}_client");

    match (rust_framework, template) {
        (RustFramework::Mollusk, Template::Minimal | Template::Full) => {
            format!(
                r#"use mollusk_svm::{{program::keyed_account_for_system_program, Mollusk}};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::Instruction;

use {client_crate}::InitializeInstruction;

fn setup() -> Mollusk {{
    Mollusk::new(&crate::ID, "target/deploy/{libname}")
}}

#[test]
fn test_initialize() {{
    let mollusk = setup();
    let (system_program, system_program_account) = keyed_account_for_system_program();

    let payer = Address::new_unique();
    let payer_account = Account::new(10_000_000_000, 0, &system_program);

    let instruction: Instruction = InitializeInstruction {{
        payer,
        system_program,
    }}
    .into();

    let result = mollusk.process_instruction(
        &instruction,
        &[
            (payer, payer_account),
            (system_program, system_program_account),
        ],
    );

    assert!(
        result.program_result.is_ok(),
        "initialize failed: {{:?}}",
        result.program_result,
    );
}}
"#
            )
        }
        (RustFramework::QuasarSVM, Template::Minimal | Template::Full) => {
            format!(
                r#"use quasar_svm::{{Account, Instruction, Pubkey, QuasarSvm}};
use solana_address::Address;

use {client_crate}::InitializeInstruction;

fn setup() -> QuasarSvm {{
    let elf = include_bytes!("../target/deploy/{libname}.so");
    QuasarSvm::new()
        .with_program(&Pubkey::from(crate::ID), elf)
}}

#[test]
fn test_initialize() {{
    let mut svm = setup();

    let payer = Pubkey::new_unique();

    let instruction: Instruction = InitializeInstruction {{
        payer: Address::from(payer.to_bytes()),
        system_program: Address::from(quasar_svm::system_program::ID.to_bytes()),
    }}
    .into();

    let result = svm.process_instruction(
        &instruction,
        &[Account {{
            address: payer,
            lamports: 10_000_000_000,
            data: vec![],
            owner: quasar_svm::system_program::ID,
            executable: false,
        }}],
    );

    result.assert_success();
}}
"#
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            env,
            path::PathBuf,
            sync::Mutex,
            time::{SystemTime, UNIX_EPOCH},
        },
    };

    static PATH_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn initialize_git_repo_runs_init_add_and_commit() {
        let _guard = PATH_LOCK.lock().unwrap();
        let sandbox = create_test_sandbox("success");
        let _env = TestGitEnv::new(&sandbox, None);
        let root = sandbox.join("repo");
        fs::create_dir_all(&root).unwrap();

        let ok = initialize_git_repo(&root, GitSetup::InitializeAndCommit);

        assert!(ok);
        assert_eq!(
            read_git_log(&sandbox),
            vec![
                "init --quiet",
                "add .",
                "commit -am chore: initial commit --quiet",
            ]
        );
    }

    #[test]
    fn initialize_git_repo_can_skip_initial_commit() {
        let _guard = PATH_LOCK.lock().unwrap();
        let sandbox = create_test_sandbox("init-only");
        let _env = TestGitEnv::new(&sandbox, None);
        let root = sandbox.join("repo");
        fs::create_dir_all(&root).unwrap();

        let ok = initialize_git_repo(&root, GitSetup::Initialize);

        assert!(ok);
        assert_eq!(read_git_log(&sandbox), vec!["init --quiet"]);
    }

    #[test]
    fn initialize_git_repo_stops_when_git_init_fails() {
        let _guard = PATH_LOCK.lock().unwrap();
        let sandbox = create_test_sandbox("fail-init");
        let _env = TestGitEnv::new(&sandbox, Some("init"));
        let root = sandbox.join("repo");
        fs::create_dir_all(&root).unwrap();

        let ok = initialize_git_repo(&root, GitSetup::InitializeAndCommit);

        assert!(!ok);
        assert_eq!(read_git_log(&sandbox), vec!["init --quiet"]);
    }

    fn create_test_sandbox(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "quasar-init-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        dir
    }

    fn read_git_log(sandbox: &Path) -> Vec<String> {
        fs::read_to_string(sandbox.join("git.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.to_string())
            .collect()
    }

    struct TestGitEnv {
        old_path: Option<std::ffi::OsString>,
        old_log: Option<std::ffi::OsString>,
        old_fail_on: Option<std::ffi::OsString>,
    }

    impl TestGitEnv {
        fn new(sandbox: &Path, fail_on: Option<&str>) -> Self {
            let bin_dir = sandbox.join("bin");
            let log_path = sandbox.join("git.log");
            write_fake_git(&bin_dir.join("git"));

            let old_path = env::var_os("PATH");
            let old_log = env::var_os("QUASAR_TEST_GIT_LOG");
            let old_fail_on = env::var_os("QUASAR_TEST_GIT_FAIL_ON");

            let mut path = std::ffi::OsString::new();
            path.push(bin_dir.as_os_str());
            path.push(":");
            if let Some(existing) = &old_path {
                path.push(existing);
            }

            // Safety: tests hold PATH_LOCK, so process-global env mutation stays
            // serialized.
            unsafe {
                env::set_var("PATH", path);
                env::set_var("QUASAR_TEST_GIT_LOG", &log_path);
            }
            if let Some(cmd) = fail_on {
                // Safety: tests hold PATH_LOCK, so process-global env mutation stays
                // serialized.
                unsafe {
                    env::set_var("QUASAR_TEST_GIT_FAIL_ON", cmd);
                }
            } else {
                // Safety: tests hold PATH_LOCK, so process-global env mutation stays
                // serialized.
                unsafe {
                    env::remove_var("QUASAR_TEST_GIT_FAIL_ON");
                }
            }

            Self {
                old_path,
                old_log,
                old_fail_on,
            }
        }
    }

    impl Drop for TestGitEnv {
        fn drop(&mut self) {
            // Safety: tests hold PATH_LOCK, so process-global env mutation stays
            // serialized.
            unsafe {
                restore_env_var("PATH", self.old_path.as_ref());
                restore_env_var("QUASAR_TEST_GIT_LOG", self.old_log.as_ref());
                restore_env_var("QUASAR_TEST_GIT_FAIL_ON", self.old_fail_on.as_ref());
            }
        }
    }

    unsafe fn restore_env_var(key: &str, value: Option<&std::ffi::OsString>) {
        if let Some(value) = value {
            env::set_var(key, value);
        } else {
            env::remove_var(key);
        }
    }

    fn write_fake_git(path: &Path) {
        fs::write(
            path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$QUASAR_TEST_GIT_LOG\"\nif [ \"$1\" = \
             \"$QUASAR_TEST_GIT_FAIL_ON\" ]; then\n  exit 1\nfi\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Static templates
// ---------------------------------------------------------------------------

const GITIGNORE: &str = "\
# Build artifacts
/target

# Lock files
Cargo.lock
package-lock.json
pnpm-lock.yaml
yarn.lock
bun.lockb

# Dependencies
node_modules

# Environment
.env
.env.*

# OS
.DS_Store
";

const CARGO_CONFIG: &str = r#"[unstable]
build-std = ["core", "alloc"]

[target.bpfel-unknown-none]
rustflags = [
"--cfg", "target_os=\"solana\"",
"--cfg", "feature=\"mem_unaligned\"",
"-C", "linker=sbpf-linker",
"-C", "panic=abort",
"-C", "relocation-model=static",
"-C", "link-arg=--disable-memory-builtins",
"-C", "link-arg=--llvm-args=--bpf-stack-size=4096",
"-C", "link-arg=--disable-expand-memcpy-in-order",
"-C", "link-arg=--export=entrypoint",
"-C", "target-cpu=v2",
]
[alias]
build-bpf = "build --release --target bpfel-unknown-none"
"#;

const INSTRUCTIONS_MOD: &str = r#"mod initialize;
pub use initialize::*;
"#;

const INSTRUCTION_INITIALIZE: &str = r#"use quasar_lang::prelude::*;

#[derive(Accounts)]
pub struct Initialize<'info> {
    pub payer: &'info mut Signer,
    pub system_program: &'info Program<System>,
}

impl<'info> Initialize<'info> {
    #[inline(always)]
    pub fn initialize(&self) -> Result<(), ProgramError> {
        Ok(())
    }
}
"#;

const STATE_RS: &str = r#"use quasar_lang::prelude::*;

#[account(discriminator = 1)]
pub struct MyAccount {
    pub authority: Address,
    pub value: u64,
}
"#;

const ERRORS_RS: &str = r#"use quasar_lang::prelude::*;

#[error_code]
pub enum MyError {
    Unauthorized,
}
"#;

const TS_TEST_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "es2020",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "types": ["node"]
  },
  "include": ["tests/*.test.ts"]
}
"#;
