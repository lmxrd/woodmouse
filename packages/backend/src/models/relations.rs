//! Relations the generator cannot write: an actor that is not a foreign key (the audit log keeps what it
//! recorded even when a user is gone) or that is one of two links to the same table (a notification's recipient
//! is `user_id`, its actor is `actor_id`). They let `load_one(users::Entity, db)` find the actor.

use sea_orm::entity::prelude::*;

use super::{audit_logs, notifications, users};

impl Related<users::Entity> for audit_logs::Entity {
    fn to() -> RelationDef {
        audit_logs::Entity::belongs_to(users::Entity)
            .from(audit_logs::Column::ActorId)
            .to(users::Column::Id)
            .into()
    }
}

impl Related<users::Entity> for notifications::Entity {
    fn to() -> RelationDef {
        notifications::Entity::belongs_to(users::Entity)
            .from(notifications::Column::ActorId)
            .to(users::Column::Id)
            .into()
    }
}
