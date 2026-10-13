//! Generate the OpenAPI specification with utoipa.
//!
//! Run with `cargo specs` (alias in `.cargo/config.toml`). Writes
//! `openapi.json` and `openapi.yaml` to `target/specs/`.

use std::path::PathBuf;

use woodmouse::routes;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/specs");
    std::fs::create_dir_all(&dir)?;

    // The router is built without state; only the OpenAPI half is used.
    let (_, openapi) = routes::api().split_for_parts();
    std::fs::write(dir.join("openapi.json"), openapi.to_pretty_json()?)?;
    std::fs::write(dir.join("openapi.yaml"), openapi.to_yaml()?)?;
    println!("wrote {}/openapi.json and openapi.yaml", dir.display());
    Ok(())
}
