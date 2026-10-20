//! Generate the OpenAPI specification with utoipa.
//!
//! Run with `mise run gen:spec` (or `cargo run --package api --bin specs`). Writes
//! `openapi.json` and `openapi.yaml` to the workspace's `target/specs/`.
//!
//! The document is built with no state (`routes::api_doc`): no database
//! is opened — no query could run anyway, only the OpenAPI half of the
//! routes is used.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This crate is `api/`; the specs go to the workspace root's `target/`, where the SDK and CI read them.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/specs");
    std::fs::create_dir_all(&dir)?;

    let openapi = api::routes::api_doc();
    std::fs::write(dir.join("openapi.json"), openapi.to_pretty_json()?)?;
    std::fs::write(dir.join("openapi.yaml"), openapi.to_yaml()?)?;
    println!("wrote {}/openapi.json and openapi.yaml", dir.display());
    Ok(())
}
