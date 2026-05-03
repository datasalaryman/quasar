use {
    quasar_cli::idl,
    serde_json::Value,
    std::{
        error::Error,
        fs,
        path::{Path, PathBuf},
        process::Command,
    },
    tempfile::tempdir,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn fixture_program() -> PathBuf {
    workspace_root().join("examples/multisig")
}

fn run_command(cmd: &mut Command) -> Result<(), Box<dyn Error>> {
    let output = cmd.output()?;
    if output.status.success() {
        return Ok(());
    }

    let mut message = String::new();
    message.push_str(&format!("command failed: {:?}\n", cmd));
    if !output.stdout.is_empty() {
        message.push_str("stdout:\n");
        message.push_str(&String::from_utf8_lossy(&output.stdout));
        message.push('\n');
    }
    if !output.stderr.is_empty() {
        message.push_str("stderr:\n");
        message.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Err(message.into())
}

fn compile_rust_client(client_dir: &Path) -> Result<(), Box<dyn Error>> {
    run_command(
        Command::new("cargo")
            .arg("check")
            .arg("--quiet")
            .current_dir(client_dir),
    )
}

fn compile_python_client(client_dir: &Path) -> Result<(), Box<dyn Error>> {
    run_command(
        Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg("__init__.py")
            .arg("client.py")
            .current_dir(client_dir),
    )
}

fn compile_go_client(client_dir: &Path) -> Result<(), Box<dyn Error>> {
    run_command(
        Command::new("go")
            .arg("mod")
            .arg("tidy")
            .current_dir(client_dir),
    )?;
    run_command(
        Command::new("go")
            .arg("build")
            .arg("./...")
            .current_dir(client_dir),
    )
}

fn compile_typescript_client(client_dir: &Path) -> Result<(), Box<dyn Error>> {
    // The smoke test validates generated client type-checking, not npm's ability
    // to resolve a GitHub-hosted dependency transport on GitHub runners.
    let package_json_path = client_dir.join("package.json");
    let mut package_json: Value = serde_json::from_str(&fs::read_to_string(&package_json_path)?)?;
    package_json["dependencies"]["@solana/web3.js"] = serde_json::json!("^1.98.4");
    fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json)? + "\n",
    )?;

    fs::write(
        client_dir.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "erasableSyntaxOnly": true,
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "noEmit": true,
    "skipLibCheck": true,
    "strict": true,
    "target": "ES2022"
  },
  "include": ["web3.ts", "kit.ts"]
}
"#,
    )?;

    run_command(
        Command::new("npm")
            .arg("install")
            .arg("--package-lock=false")
            .arg("--ignore-scripts")
            .arg("--no-audit")
            .arg("--no-fund")
            .arg("typescript")
            .arg("@types/node")
            .current_dir(client_dir),
    )?;

    run_command(
        Command::new("npx")
            .arg("tsc")
            .arg("-p")
            .arg("tsconfig.json")
            .current_dir(client_dir),
    )
}

#[test]
fn generated_clients_compile_from_fresh_project() -> Result<(), Box<dyn Error>> {
    let fixture = fixture_program();

    let temp = tempdir()?;
    let clients_path = temp.path().join("clients");
    idl::generate(&fixture, &["typescript", "python", "golang"], &clients_path)?;

    // The IDL is generated relative to the workspace; find the rust client dir
    // by convention.
    let rust_client_dir = clients_path.join("rust").join("quasar-multisig-client");
    if rust_client_dir.exists() {
        // Patch the generated Cargo.toml to use the local workspace `quasar-lang`
        // instead of the GitHub remote, so the smoke test validates against the
        // current (possibly unreleased) source.
        let cargo_toml_path = rust_client_dir.join("Cargo.toml");
        let cargo_toml = fs::read_to_string(&cargo_toml_path)?;
        let patched = cargo_toml.replace(
            "quasar-lang = { git = \"https://github.com/blueshift-gg/quasar\", branch = \
             \"master\" }",
            &format!(
                "quasar-lang = {{ path = \"{}\" }}",
                workspace_root().join("lang").display()
            ),
        );
        fs::write(&cargo_toml_path, &patched)?;
        compile_rust_client(&rust_client_dir)?;
    }

    let ts_dir = clients_path.join("typescript").join("quasar-multisig");
    if ts_dir.exists() {
        compile_typescript_client(&ts_dir)?;
    }

    let py_dir = clients_path.join("python").join("quasar-multisig");
    if py_dir.exists() {
        compile_python_client(&py_dir)?;
    }

    let go_dir = clients_path.join("golang").join("quasar_multisig");
    if go_dir.exists() {
        compile_go_client(&go_dir)?;
    }

    Ok(())
}
