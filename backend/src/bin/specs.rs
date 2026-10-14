//! Generate the OpenAPI specification with utoipa.
//!
//! Run with `cargo specs` (alias in `.cargo/config.toml`). Writes
//! `openapi.json` and `openapi.yaml` to `target/specs/`.
//!
//! Building the router requires an `AppState` for the auth layer; an
//! in-memory database stands in — no queries are executed, only the
//! OpenAPI half of the router is used.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = toasty::Db::builder()
        .models(toasty::models!(woodmouse::*))
        .connect("sqlite::memory:")
        .await?;

    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/specs");
    std::fs::create_dir_all(&dir)?;

    // Only the OpenAPI half is used; the state is inert.
    let (_, openapi) =
        woodmouse::routes::api(woodmouse::state::AppState::new(db)).split_for_parts();
    std::fs::write(dir.join("openapi.json"), openapi.to_pretty_json()?)?;
    std::fs::write(dir.join("openapi.yaml"), openapi.to_yaml()?)?;
    println!("wrote {}/openapi.json and openapi.yaml", dir.display());
    Ok(())
}
