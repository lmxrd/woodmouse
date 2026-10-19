//! The enums stored as their lowercase labels (`ActiveEnum`), which the generated entities use.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Workspace role. Variant order defines the privilege order: read < write <
/// admin. Stored as its lowercase label (`read`, `write`, `admin`), the same string the API uses.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
    EnumIter,
    DeriveActiveEnum,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::None)",
    rename_all = "lowercase"
)]
pub enum Role {
    Read,
    Write,
    Admin,
}

/// Task priority. Variant order defines the urgency order.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
    EnumIter,
    DeriveActiveEnum,
)]
#[serde(rename_all = "lowercase")]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::None)",
    rename_all = "lowercase"
)]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

/// How two tasks relate. `Blocks` and `Duplicates` read from the owning task to the related one
/// ("A blocks B"); `RelatesTo` is symmetric.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, EnumIter, DeriveActiveEnum,
)]
#[serde(rename_all = "snake_case")]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::None)",
    rename_all = "snake_case"
)]
pub enum LinkKind {
    Blocks,
    RelatesTo,
    Duplicates,
}

/// Who may see an audit entry: the workspace's activity feed (and the audit) or the audit only.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, EnumIter, DeriveActiveEnum,
)]
#[serde(rename_all = "snake_case")]
#[sea_orm(
    rs_type = "String",
    db_type = "String(StringLen::None)",
    rename_all = "snake_case"
)]
pub enum Visibility {
    Workspace,
    Admin,
}
