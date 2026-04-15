use std::collections::BTreeSet;
use std::fmt;

use clap::ValueEnum;
use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table => f.write_str("table"),
            Self::Json => f.write_str("json"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Table {
    title: Option<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    empty_message: String,
}

impl Table {
    pub fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            title: None,
            headers: headers.into_iter().map(Into::into).collect(),
            rows: Vec::new(),
            empty_message: "No rows.".to_owned(),
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_empty_message(mut self, message: impl Into<String>) -> Self {
        self.empty_message = message.into();
        self
    }

    pub fn add_row(&mut self, row: impl IntoIterator<Item = impl Into<String>>) {
        self.rows.push(row.into_iter().map(clean_cell).collect());
    }

    pub fn render(&self) -> String {
        if self.headers.is_empty() {
            return self.title.clone().unwrap_or_default();
        }

        let widths = self.column_widths();
        let border = render_border(&widths);
        let mut lines = Vec::new();

        if let Some(title) = &self.title {
            lines.push(title.clone());
        }

        lines.push(border.clone());
        lines.push(render_row(&self.headers, &widths));
        lines.push(border.clone());

        if self.rows.is_empty() {
            let width = border.chars().count().saturating_sub(4);
            lines.push(format!("| {:width$} |", self.empty_message, width = width));
        } else {
            for row in &self.rows {
                lines.push(render_row(row, &widths));
            }
        }

        lines.push(border);
        lines.join("\n")
    }

    fn column_widths(&self) -> Vec<usize> {
        let mut widths: Vec<usize> = self
            .headers
            .iter()
            .map(|header| cell_width(header))
            .collect();

        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(index) {
                    *width = (*width).max(cell_width(cell));
                }
            }
        }

        if self.rows.is_empty() && !widths.is_empty() {
            let span_width = widths.iter().sum::<usize>() + (widths.len() * 3).saturating_sub(3);
            let message_width = cell_width(&self.empty_message);
            if message_width > span_width
                && let Some(last_width) = widths.last_mut()
            {
                *last_width += message_width - span_width;
            }
        }

        widths
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotRow {
    pub storage: String,
    pub name: String,
    pub comment: String,
}

impl SnapshotRow {
    pub fn new(
        storage: impl Into<String>,
        name: impl Into<String>,
        comment: impl Into<String>,
    ) -> Self {
        Self {
            storage: storage.into(),
            name: name.into(),
            comment: comment.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailSection {
    title: String,
    value: Value,
}

impl DetailSection {
    pub fn new(title: impl Into<String>, value: Value) -> Self {
        Self {
            title: title.into(),
            value,
        }
    }
}

pub fn print_snapshots(format: OutputFormat, snapshots: &[SnapshotRow]) -> Result<()> {
    print_snapshots_with_storage(format, snapshots, false)
}

pub fn print_snapshots_with_storage(
    format: OutputFormat,
    snapshots: &[SnapshotRow],
    include_storage: bool,
) -> Result<()> {
    match format {
        OutputFormat::Table => println!("{}", render_snapshot_table(snapshots, include_storage)),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshots)?),
    }

    Ok(())
}

pub fn print_detail(format: OutputFormat, title: &str, value: &Value) -> Result<()> {
    match format {
        OutputFormat::Table => println!("{}", render_detail_section(title, value)),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
    }

    Ok(())
}

pub fn print_sections<T>(
    format: OutputFormat,
    sections: &[DetailSection],
    json_value: &T,
) -> Result<()>
where
    T: Serialize,
{
    match format {
        OutputFormat::Table => {
            let rendered = sections
                .iter()
                .map(|section| render_detail_section(&section.title, &section.value))
                .collect::<Vec<_>>()
                .join("\n\n");
            println!("{rendered}");
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(json_value)?),
    }

    Ok(())
}

fn render_snapshot_table(snapshots: &[SnapshotRow], force_storage_column: bool) -> String {
    let storage_count = snapshots
        .iter()
        .map(|snapshot| snapshot.storage.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let include_storage = force_storage_column || storage_count > 1;

    let mut table = if include_storage {
        Table::new(["Storage", "Name", "Comment"])
    } else {
        Table::new(["Name", "Comment"])
    }
    .with_empty_message("No snapshots found.");

    for snapshot in snapshots {
        if include_storage {
            table.add_row([&snapshot.storage, &snapshot.name, &snapshot.comment]);
        } else {
            table.add_row([&snapshot.name, &snapshot.comment]);
        }
    }

    table.render()
}

fn render_detail_section(title: &str, value: &Value) -> String {
    let mut table = Table::new(["Field", "Value"])
        .titled(title)
        .with_empty_message("No detail fields found.");

    let mut rows = Vec::new();
    flatten_value("", value, &mut rows);
    for (field, value) in rows {
        table.add_row([field, value]);
    }

    table.render()
}

fn flatten_value(prefix: &str, value: &Value, rows: &mut Vec<(String, String)>) {
    match value {
        Value::Object(object) => {
            if object.is_empty() && !prefix.is_empty() {
                rows.push((prefix.to_owned(), "{}".to_owned()));
                return;
            }

            for (key, nested) in object {
                let path = if prefix.is_empty() {
                    key.to_owned()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_value(&path, nested, rows);
            }
        }
        Value::Array(values) => {
            if values.is_empty() {
                rows.push((prefix_or_value(prefix), "[]".to_owned()));
                return;
            }

            for (index, nested) in values.iter().enumerate() {
                let path = if prefix.is_empty() {
                    format!("[{index}]")
                } else {
                    format!("{prefix}[{index}]")
                };
                flatten_value(&path, nested, rows);
            }
        }
        _ => rows.push((prefix_or_value(prefix), json_cell(value))),
    }
}

fn prefix_or_value(prefix: &str) -> String {
    if prefix.is_empty() {
        "value".to_owned()
    } else {
        prefix.to_owned()
    }
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn render_border(widths: &[usize]) -> String {
    let mut border = String::from("+");
    for width in widths {
        border.push_str(&"-".repeat(*width + 2));
        border.push('+');
    }
    border
}

fn render_row(cells: &[String], widths: &[usize]) -> String {
    let mut row = String::from("|");
    for (index, width) in widths.iter().enumerate() {
        let cell = cells.get(index).map(String::as_str).unwrap_or("");
        row.push(' ');
        row.push_str(cell);
        row.push_str(&" ".repeat(width.saturating_sub(cell_width(cell)) + 1));
        row.push('|');
    }
    row
}

fn clean_cell(value: impl Into<String>) -> String {
    value.into().replace('\n', "\\n").replace('\r', "\\r")
}

fn cell_width(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{SnapshotRow, render_detail_section, render_snapshot_table};

    #[test]
    fn renders_snapshot_rows_as_aligned_table() {
        let rendered = render_snapshot_table(
            &[
                SnapshotRow::new("NAS01", "snap-1", "first"),
                SnapshotRow::new("NAS02", "snap-longer", "second"),
            ],
            false,
        );

        assert!(rendered.contains("| Storage | Name        | Comment |"));
        assert!(rendered.contains("| NAS01   | snap-1      | first   |"));
        assert!(rendered.contains("| NAS02   | snap-longer | second  |"));
    }

    #[test]
    fn can_force_storage_column_for_all_storage_snapshot_lists() {
        let rendered = render_snapshot_table(&[SnapshotRow::new("NAS01", "snap-1", "first")], true);

        assert!(rendered.contains("| Storage | Name   | Comment |"));
        assert!(rendered.contains("| NAS01   | snap-1 | first   |"));
    }

    #[test]
    fn renders_single_storage_snapshot_rows_without_repeated_storage_column() {
        let rendered =
            render_snapshot_table(&[SnapshotRow::new("NAS01", "snap-1", "first")], false);

        assert!(rendered.contains("| Name   | Comment |"));
        assert!(!rendered.contains("Storage"));
        assert!(rendered.contains("| snap-1 | first   |"));
    }

    #[test]
    fn renders_empty_table_with_aligned_empty_message() {
        let rendered = render_snapshot_table(&[], false);

        assert!(rendered.contains("| No snapshots found. |"));
        assert_eq!(
            rendered.lines().next().map(str::len),
            rendered.lines().nth(3).map(str::len)
        );
    }

    #[test]
    fn renders_json_detail_as_key_value_table() {
        let rendered = render_detail_section(
            "Volume Info",
            &json!({
                "name": "nasvol",
                "svm": { "name": "svm1" },
                "tags": ["prod", "backup"]
            }),
        );

        assert!(rendered.contains("Volume Info"));
        assert!(rendered.contains("| name     | nasvol |"));
        assert!(rendered.contains("| svm.name | svm1   |"));
        assert!(rendered.contains("| tags[0]  | prod   |"));
        assert!(rendered.contains("| tags[1]  | backup |"));
    }
}
