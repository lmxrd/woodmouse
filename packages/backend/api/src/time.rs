//! Dates and times live in the `entity` crate (the stored `DateTime` is a column type); re-exported here
//! so the rest of the API keeps its short `crate::time` paths.

pub use entity::time::*;
