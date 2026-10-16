//! Schema migrations: `cargo run --bin migrate -- generate <name>` writes the next migration into
//! `backend/toasty/` when the models differ from the last snapshot. The server applies the pending ones at
//! startup. Run it after changing `src/models.rs`, and commit the new files.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("generate") => {
            let name = args.get(1).ok_or("usage: migrate generate <name>")?;
            // Only the models are needed; an in-memory database stands in.
            let db = toasty::Db::builder()
                .models(toasty::models!(woodmouse::*))
                .connect("sqlite::memory:")
                .await?;
            match woodmouse::migrations::generate(&db, name)? {
                Some(file) => println!("wrote toasty/migrations/{file}"),
                None => println!("the models match the last snapshot: nothing to generate"),
            }
            Ok(())
        }
        _ => Err("usage: migrate generate <name>".into()),
    }
}
