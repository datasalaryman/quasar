use {
    crate::{error::CliResult, IdlCommand},
    quasar_idl::{codegen, parser},
    std::path::{Path, PathBuf},
};

/// Generate IDL and client crates for a program.
///
/// Outputs:
/// - `target/idl/<name>.idl.json` (always)
/// - `target/client/rust/<name>-client/` when languages contains "rust"
/// - `target/client/typescript/<name>/` when languages contains "typescript"
/// - `target/client/python/<name>/` when languages contains "python"
/// - `target/client/golang/<name>/` when languages contains "golang"
pub fn generate(crate_path: &Path, languages: &[&str]) -> CliResult {
    // Parse the program
    let parsed = parser::parse_program(crate_path);

    // Generate Rust client code before build_idl consumes parsed
    let (client_code, client_cargo_toml) = if languages.contains(&"rust") {
        (
            Some(codegen::rust::generate_client(&parsed)),
            Some(codegen::rust::generate_cargo_toml(&parsed.crate_name, &parsed.version)),
        )
    } else {
        (None, None)
    };

    // Build the IDL
    let idl = parser::build_idl(parsed);

    // Write IDL JSON to target/idl/
    let idl_dir = PathBuf::from("target").join("idl");
    std::fs::create_dir_all(&idl_dir)?;

    let idl_path = idl_dir.join(format!("{}.idl.json", idl.metadata.name));
    let json = serde_json::to_string_pretty(&idl)
        .map_err(|e| anyhow::anyhow!("failed to serialize IDL: {e}"))?;
    std::fs::write(&idl_path, &json)?;

    // TypeScript client
    if languages.contains(&"typescript") {
        let ts_code = codegen::typescript::generate_ts_client(&idl);
        let ts_kit_code = codegen::typescript::generate_ts_client_kit(&idl);

        // Write TypeScript clients to target/client/typescript/<name>/
        let ts_dir = PathBuf::from("target")
            .join("client")
            .join("typescript")
            .join(&idl.metadata.name);
        std::fs::create_dir_all(&ts_dir)?;
        std::fs::write(ts_dir.join("web3.ts"), &ts_code)?;
        std::fs::write(ts_dir.join("kit.ts"), &ts_kit_code)?;

        // Write package.json for the TS client
        let needs_codecs =
            !idl.types.is_empty() || idl.instructions.iter().any(|ix| !ix.args.is_empty());
        let codecs_dep = if needs_codecs {
            "\n    \"@solana/codecs\": \"^6.2.0\","
        } else {
            ""
        };
        let ts_package_json = format!(
            r#"{{
  "name": "{crate_name}-client",
  "version": "{version}",
  "private": true,
  "exports": {{
    "./web3.js": "./web3.ts",
    "./kit": "./kit.ts"
  }},
  "dependencies": {{{codecs_dep}
    "@solana/kit": "^6.0.0",
    "@solana/web3.js": "github:blueshift-gg/web3.js#v2"
  }}
}}
"#,
            crate_name = idl.metadata.crate_name,
            version = idl.metadata.version,
        );
        std::fs::write(ts_dir.join("package.json"), &ts_package_json)?;
    }

    // Rust client
    if let (Some(code), Some(cargo_toml)) = (client_code, client_cargo_toml) {
        let crate_name = &idl.metadata.crate_name;
        let client_dir = PathBuf::from("target")
            .join("client")
            .join("rust")
            .join(format!("{}-client", crate_name));
        let client_src_dir = client_dir.join("src");
        std::fs::create_dir_all(&client_src_dir)?;

        std::fs::write(client_dir.join("Cargo.toml"), &cargo_toml)?;
        std::fs::write(client_src_dir.join("lib.rs"), &code)?;
    }

    // Python client
    if languages.contains(&"python") {
        let py_code = codegen::python::generate_python_client(&idl);
        let crate_name = &idl.metadata.crate_name;
        let py_dir = PathBuf::from("target")
            .join("client")
            .join("python")
            .join(crate_name);
        std::fs::create_dir_all(&py_dir)?;

        std::fs::write(py_dir.join("client.py"), &py_code)?;
        std::fs::write(py_dir.join("__init__.py"), format!(
            "from .client import *  # noqa: F401,F403\n"
        ))?;
    }

    // Go client
    if languages.contains(&"golang") {
        let go_code = codegen::golang::generate_go_client(&idl);
        let crate_name = &idl.metadata.crate_name;
        let go_pkg = crate_name.replace('-', "_");
        let go_dir = PathBuf::from("target")
            .join("client")
            .join("golang")
            .join(&go_pkg);
        std::fs::create_dir_all(&go_dir)?;

        std::fs::write(go_dir.join("client.go"), &go_code)?;
        std::fs::write(
            go_dir.join("go.mod"),
            codegen::golang::generate_go_mod(&go_pkg),
        )?;
    }

    Ok(())
}

pub fn run(command: IdlCommand) -> CliResult {
    let crate_path = &command.crate_path;

    if !crate_path.exists() {
        eprintln!("Error: path does not exist: {}", crate_path.display());
        std::process::exit(1);
    }

    // `quasar idl` generates all available languages
    generate(crate_path, &["rust", "typescript", "python", "golang"])?;
    println!("  {}", crate::style::success("IDL generated"));
    Ok(())
}
