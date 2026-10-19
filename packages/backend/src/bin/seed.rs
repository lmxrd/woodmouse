//! `mise run seed` (or `cargo run --bin seed`): mock data for development. It fills an empty database with
//! three people, a workspace with projects, tasks, comments and wiki pages. Tasks, comments and wiki pages go
//! through the same content functions the API uses, so their revisions, activity entries and search entries
//! are there too; the plain rows (people, projects, lists, labels, sprints) are inserted directly.

use tracing_subscriber::EnvFilter;
use woodmouse::audit::{self, Entry};
use woodmouse::auth::hash_password;
use woodmouse::blocks::{Attrs, BlockData, BlockKind, Span};
use woodmouse::db::Db;
use woodmouse::db::prelude::*;
use woodmouse::error::ApiError;
use woodmouse::models::users;
use woodmouse::models::{Label, List, Priority, Project, Role, User, WikiPage};
use woodmouse::models::{
    comments, labels, lists, projects, sprints, tasks, wiki_pages, workspace_members, workspaces,
};
use woodmouse::routes::tasks::{CreateTaskRequest, create_task_in};
use woodmouse::state::AppState;
use woodmouse::time::{self, DateTime};
use woodmouse::{pages, revisions};

type Id = uuid::Uuid;

/// The password of every seeded account: development data only, so it is as simple as can be.
const PASSWORD: &str = "password";

/// A day `offset` days from today (midnight UTC).
fn day(offset: i64) -> DateTime {
    time::add_seconds(time::to_day(time::now()), offset * 86_400)
}

fn block(kind: BlockKind, level: Option<u8>, text: &str) -> BlockData {
    BlockData {
        kind,
        attrs: Attrs {
            level,
            ..Attrs::default()
        },
        spans: vec![Span {
            text: Some(text.to_string()),
            ..Span::default()
        }],
    }
}

fn heading(text: &str) -> BlockData {
    block(BlockKind::Heading, Some(2), text)
}

fn paragraph(text: &str) -> BlockData {
    block(BlockKind::Paragraph, None, text)
}

fn bullet(text: &str) -> BlockData {
    block(BlockKind::BulletItem, None, text)
}

async fn user(db: &impl Db, email: &str, name: &str, admin: bool) -> Result<User, ApiError> {
    Ok(users::ActiveModel {
        id: Set(Id::now_v7()),
        email: Set(email.to_string()),
        name: Set(name.to_string()),
        password_hash: Set(hash_password(PASSWORD)?),
        super_admin: Set(admin),
        active: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

async fn project(
    db: &impl Db,
    workspace: Id,
    name: &str,
    description: &str,
) -> Result<Project, ApiError> {
    Ok(projects::ActiveModel {
        id: Set(Id::now_v7()),
        workspace_id: Set(workspace),
        name: Set(name.to_string()),
        description: Set(description.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// Lists are ordered by `position`, 20 apart.
async fn lists(db: &impl Db, project: Id, names: &[&str]) -> Result<Vec<List>, ApiError> {
    let mut out = vec![];
    for (i, name) in names.iter().enumerate() {
        out.push(
            lists::ActiveModel {
                id: Set(Id::now_v7()),
                project_id: Set(project),
                name: Set(name.to_string()),
                position: Set((i as i64 + 1) * 20),
                ..Default::default()
            }
            .insert(db)
            .await?,
        );
    }
    Ok(out)
}

fn task(title: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        title: title.to_string(),
        description: String::new(),
        priority: None,
        start_date: None,
        due_date: None,
        sprint_id: None,
        estimate_minutes: None,
        spent_minutes: None,
        assignee_ids: vec![],
        label_ids: vec![],
    }
}

/// A wiki page with its content, written the way a save through the API writes it.
async fn page(
    db: &impl Db,
    workspace: Id,
    parent: Option<Id>,
    position: i64,
    author: Id,
    title: &str,
    blocks: &[BlockData],
) -> Result<WikiPage, ApiError> {
    let now = time::now();
    let tx = db.begin().await?;
    let mut page = wiki_pages::ActiveModel {
        id: Set(Id::now_v7()),
        workspace_id: Set(workspace),
        parent_id: Set(parent),
        title: Set(title.to_string()),
        position: Set(position),
        content: Set(woodmouse::blocks::encode(&[])),
        created_by: Set(author),
        created_at: Set(now),
        updated_by: Set(author),
        updated_at: Set(now),
        version: Set(0),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    revisions::record_page(
        &tx,
        page.id,
        author,
        &revisions::PageContent {
            title: title.to_string(),
            blocks: vec![],
        },
        true,
    )
    .await?;
    pages::save_content(&tx, &mut page, author, blocks, true).await?;
    tx.commit().await?;
    Ok(page)
}

async fn run(state: &AppState) -> Result<(), ApiError> {
    let writer = state.writer().await?;
    let db = &*writer;
    let admin = user(db, "admin@woodmouse.test", "Alex Admin", true).await?;
    let sara = user(db, "sara@woodmouse.test", "Sara Writer", false).await?;
    let ola = user(db, "ola@woodmouse.test", "Ola Reader", false).await?;

    let ws = workspaces::ActiveModel {
        id: Set(Id::now_v7()),
        name: Set("Home".to_string()),
        description: Set("Household projects and notes".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    for (member, role) in [
        (&admin, Role::Admin),
        (&sara, Role::Write),
        (&ola, Role::Read),
    ] {
        workspace_members::ActiveModel {
            user_id: Set(member.id),
            workspace_id: Set(ws.id),
            role: Set(role),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    audit::record(
        db,
        Entry::admin(Some(admin.id), "workspace_created")
            .target("workspace", ws.id)
            .in_workspace(ws.id)
            .data(serde_json::json!({ "name": ws.name })),
    )
    .await?;

    let mut labels: Vec<Label> = vec![];
    for (name, color) in [
        ("Urgent", "#dc2626"),
        ("Shopping", "#2563eb"),
        ("Outdoors", "#16a34a"),
    ] {
        labels.push(
            labels::ActiveModel {
                id: Set(Id::now_v7()),
                workspace_id: Set(ws.id),
                name: Set(name.to_string()),
                color: Set(color.to_string()),
                ..Default::default()
            }
            .insert(db)
            .await?,
        );
    }

    // House renovation: lists, dated tasks, subtasks, comments.
    let house = project(
        db,
        ws.id,
        "House renovation",
        "Kitchen first, then the hallway",
    )
    .await?;
    let house_lists = lists(db, house.id, &["To do", "Doing", "Waiting"]).await?;
    let sprint = sprints::ActiveModel {
        id: Set(Id::now_v7()),
        project_id: Set(house.id),
        name: Set("Kitchen sprint".to_string()),
        goal: Set("Order everything for the kitchen".to_string()),
        start_date: Set(day(-3)),
        end_date: Set(day(11)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let oak = create_task_in(
        db,
        ws.id,
        house.id,
        Some(house_lists[1].id),
        None,
        admin.id,
        CreateTaskRequest {
            description: "Check the **delivery date** first.".to_string(),
            priority: Some(Priority::High),
            start_date: Some(day(-2)),
            due_date: Some(day(3)),
            sprint_id: Some(sprint.id),
            estimate_minutes: Some(480),
            spent_minutes: Some(150),
            assignee_ids: vec![sara.id],
            label_ids: vec![labels[1].id],
            ..task("Order oak worktop")
        },
    )
    .await?;
    for title in ["Measure the counter", "Compare three quotes"] {
        create_task_in(
            db,
            ws.id,
            house.id,
            Some(house_lists[1].id),
            Some(oak.id),
            admin.id,
            task(title),
        )
        .await?;
    }
    let comment = comments::ActiveModel {
        id: Set(Id::now_v7()),
        task_id: Set(oak.id),
        author_id: Set(sara.id),
        body: Set("Back in stock on the 17th.".to_string()),
        created_at: Set(time::now()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    revisions::record_comment(db, comment.id, sara.id, &comment.body).await?;
    create_task_in(
        db,
        ws.id,
        house.id,
        Some(house_lists[0].id),
        None,
        admin.id,
        CreateTaskRequest {
            priority: Some(Priority::Medium),
            start_date: Some(day(5)),
            due_date: Some(day(9)),
            ..task("Paint the hallway")
        },
    )
    .await?;
    create_task_in(
        db,
        ws.id,
        house.id,
        Some(house_lists[0].id),
        None,
        admin.id,
        CreateTaskRequest {
            priority: Some(Priority::Urgent),
            due_date: Some(day(1)),
            label_ids: vec![labels[0].id],
            ..task("Book the electrician")
        },
    )
    .await?;
    let done = create_task_in(
        db,
        ws.id,
        house.id,
        Some(house_lists[2].id),
        None,
        admin.id,
        CreateTaskRequest {
            due_date: Some(day(-4)),
            ..task("Pick tiles")
        },
    )
    .await?;
    tasks::ActiveModel {
        id: Unchanged(done.id),
        completed_at: Set(Some(time::now())),
        ..Default::default()
    }
    .update(db)
    .await?;
    // No list: it lands in the project's inbox.
    create_task_in(
        db,
        ws.id,
        house.id,
        None,
        None,
        admin.id,
        task("Ask the neighbours about the skip"),
    )
    .await?;

    // Garden.
    let garden = project(db, ws.id, "Garden", "").await?;
    let spring = lists(db, garden.id, &["Spring"]).await?;
    for (title, due) in [
        ("Prune the roses", 6),
        ("Buy compost", 2),
        ("Fix the fence", 14),
    ] {
        create_task_in(
            db,
            ws.id,
            garden.id,
            Some(spring[0].id),
            None,
            admin.id,
            CreateTaskRequest {
                due_date: Some(day(due)),
                label_ids: vec![labels[2].id],
                ..task(title)
            },
        )
        .await?;
    }

    // Wiki.
    let home = page(
        db,
        ws.id,
        None,
        20,
        admin.id,
        "Household",
        &[
            heading("Welcome"),
            paragraph("Everything we need to run the house, in one place."),
            bullet("Contacts and contractors"),
            bullet("Budget"),
        ],
    )
    .await?;
    for (i, (title, text)) in [
        ("Contractors", "Electrician: call before 9."),
        ("Budget", "Kitchen: 12 000. Hallway: 3 500."),
    ]
    .into_iter()
    .enumerate()
    {
        page(
            db,
            ws.id,
            Some(home.id),
            (i as i64 + 1) * 20,
            admin.id,
            title,
            &[heading(title), paragraph(text)],
        )
        .await?;
    }

    println!("Seeded. Sign in with any of these (password: {PASSWORD}):");
    println!(
        "  admin@woodmouse.test (super admin)\n  sara@woodmouse.test (write)\n  ola@woodmouse.test (read)"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:woodmouse.db".to_string());
    let state = AppState::open(&db_url).await?;
    let has_users = !users::Entity::find()
        .all(state.db())
        .await
        .map_err(|e| format!("could not read the database: {e:?}"))?
        .is_empty();
    if has_users {
        return Err("the database already has users: run `mise run clean` to start over".into());
    }
    run(&state)
        .await
        .map_err(|e| format!("seeding failed: {e:?}"))?;
    Ok(())
}
