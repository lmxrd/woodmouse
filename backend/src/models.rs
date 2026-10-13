use serde::Serialize;
use utoipa::ToSchema;

/// A toasty model. The derive macro infers the table schema from the struct
/// and generates query/create/update methods (`Todo::all`, `Todo::get_by_id`,
/// `toasty::create!(Todo { .. })`, ...).
#[derive(Debug, Serialize, ToSchema, toasty::Model)]
pub struct Todo {
    /// Primary key; auto-filled on insert with a time-ordered UUID v7.
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub title: String,

    pub done: bool,
}
