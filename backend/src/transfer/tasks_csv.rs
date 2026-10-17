//! Tasks as CSV.

use crate::error::ApiError;

use super::*;

const CSV_COLUMNS: [&str; 19] = [
    "id",
    "parent",
    "project",
    "project_description",
    "list",
    "sprint",
    "sprint_start",
    "sprint_end",
    "sprint_goal",
    "title",
    "description",
    "priority",
    "start_date",
    "due_date",
    "estimate_minutes",
    "spent_minutes",
    "completed",
    "labels",
    "assignees",
];

/// The tasks of the given projects as CSV: one row per task, with the project, list and sprint on every
/// row, so the file alone can set everything up again. Labels and assignee emails are `;` separated.
pub fn tasks_csv(projects: &[ExportProject]) -> Result<String, ApiError> {
    let mut w = csv::Writer::from_writer(vec![]);
    let fail = |e: &dyn std::fmt::Display| {
        tracing::error!(%e, "writing CSV");
        ApiError::internal("could not write the CSV")
    };
    w.write_record(CSV_COLUMNS).map_err(|e| fail(&e))?;
    for p in projects {
        for t in &p.tasks {
            let sprint = t
                .sprint
                .as_ref()
                .and_then(|name| p.sprints.iter().find(|s| &s.name == name));
            w.write_record([
                t.id.to_string(),
                t.parent.map(|p| p.to_string()).unwrap_or_default(),
                p.name.clone(),
                p.description.clone(),
                t.list.clone().unwrap_or_default(),
                t.sprint.clone().unwrap_or_default(),
                sprint
                    .and_then(|s| s.start_date.clone())
                    .unwrap_or_default(),
                sprint.and_then(|s| s.end_date.clone()).unwrap_or_default(),
                sprint.map(|s| s.goal.clone()).unwrap_or_default(),
                t.title.clone(),
                t.description.clone(),
                t.priority.clone().unwrap_or_default(),
                t.start_date.clone().unwrap_or_default(),
                t.due_date.clone().unwrap_or_default(),
                t.estimate_minutes
                    .map(|m| m.to_string())
                    .unwrap_or_default(),
                t.spent_minutes.map(|m| m.to_string()).unwrap_or_default(),
                t.completed.clone().unwrap_or_default(),
                t.labels.join(";"),
                t.assignees.join(";"),
            ])
            .map_err(|e| fail(&e))?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| fail(&e))?).map_err(|e| fail(&e))
}

/// Read a tasks CSV into projects. `default_project` names the project for rows without one. Rows that
/// cannot be read are reported, not fatal.
pub fn parse_tasks_csv(
    text: &str,
    default_project: Option<&str>,
    report: &mut ImportReport,
) -> Result<Vec<ExportProject>, ApiError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| ApiError::bad_request(format!("the CSV has no header row: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    if !headers.iter().any(|h| h == "title") {
        return Err(ApiError::bad_request("the CSV needs a `title` column"));
    }
    let col = |name: &str| headers.iter().position(|h| h == name);
    let mut projects: Vec<ExportProject> = vec![];
    for (i, record) in reader.records().enumerate() {
        let row = i + 2; // the header is row 1
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                report.problems.push(format!("row {row}: {e}"));
                continue;
            }
        };
        let get = |name: &str| -> String {
            col(name)
                .and_then(|c| record.get(c))
                .map(str::trim)
                .unwrap_or_default()
                .to_string()
        };
        let some = |name: &str| Some(get(name)).filter(|v| !v.is_empty());
        if record.iter().all(|f| f.trim().is_empty()) {
            continue; // a blank row is not a problem
        }
        let title = get("title");
        if title.is_empty() {
            report.problems.push(format!("row {row}: no title"));
            continue;
        }
        let project_name = some("project")
            .or_else(|| default_project.map(str::to_string))
            .filter(|p| !p.is_empty());
        let Some(project_name) = project_name else {
            report.problems.push(format!(
                "row {row}: no project (add a `project` column or choose one)"
            ));
            continue;
        };
        let number = |name: &str| -> Result<Option<i64>, String> {
            match some(name) {
                None => Ok(None),
                Some(v) => v
                    .parse::<i64>()
                    .map(Some)
                    .map_err(|_| format!("{name} `{v}` is not a number")),
            }
        };
        let (estimate, spent) = match (number("estimate_minutes"), number("spent_minutes")) {
            (Ok(e), Ok(s)) => (e, s),
            (Err(e), _) | (_, Err(e)) => {
                report.problems.push(format!("row {row}: {e}"));
                continue;
            }
        };
        let idx = match projects.iter().position(|p| p.name == project_name) {
            Some(i) => i,
            None => {
                projects.push(ExportProject {
                    name: project_name,
                    description: get("project_description"),
                    ..Default::default()
                });
                projects.len() - 1
            }
        };
        let project = &mut projects[idx];
        if project.description.is_empty() {
            project.description = get("project_description");
        }
        let list = some("list");
        if let Some(list) = &list
            && !project.lists.contains(list)
        {
            project.lists.push(list.clone());
        }
        let sprint = some("sprint");
        if let Some(name) = &sprint
            && !project.sprints.iter().any(|s| &s.name == name)
        {
            project.sprints.push(ExportSprint {
                name: name.clone(),
                goal: get("sprint_goal"),
                start_date: some("sprint_start"),
                end_date: some("sprint_end"),
                completed_at: None,
            });
        }
        let id = some("id")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(project.tasks.len() as u32 + 1);
        let split = |v: String| -> Vec<String> {
            v.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        project.tasks.push(ExportTask {
            id,
            parent: some("parent").and_then(|v| v.parse().ok()),
            list,
            sprint,
            title,
            description: get("description"),
            priority: some("priority"),
            start_date: some("start_date"),
            due_date: some("due_date"),
            estimate_minutes: estimate,
            spent_minutes: spent,
            completed: some("completed")
                .filter(|v| !["false", "no", "0"].contains(&v.to_lowercase().as_str())),
            labels: split(get("labels")),
            assignees: split(get("assignees")),
            comments: vec![],
        });
    }
    Ok(projects)
}
