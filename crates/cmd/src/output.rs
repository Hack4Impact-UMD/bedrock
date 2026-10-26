use std::io::{self, BufRead, Write};

use anyhow::Result;
use chrono::{DateTime, Utc};
use identity::{ApiKey, NewApiKey, Project};
use serde::Serialize;
use serde_json::json;

/// Prints results either as aligned text or, with `--json`, as the serialized models.
pub struct Output {
    json: bool,
}

impl Output {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn project(&self, project: &Project) -> Result<()> {
        if self.json {
            return print_json(project);
        }

        print_fields(&project_fields(project));
        Ok(())
    }

    pub fn projects(&self, projects: &[Project]) -> Result<()> {
        if self.json {
            return print_json(projects);
        }

        print_table(
            ["ID", "SLUG", "NAME", "CREATED"],
            projects.iter().map(|p| {
                [
                    p.id.to_string(),
                    p.slug.clone(),
                    p.name.clone(),
                    format_time(&p.created_at),
                ]
            }),
        );
        Ok(())
    }

    pub fn created_project(&self, project: &Project, key: &NewApiKey) -> Result<()> {
        if self.json {
            return print_json(&json!({
                "project": project,
                "key": key.key,
                "plaintext": key.plaintext,
            }));
        }

        print_fields(&project_fields(project));
        println!();
        self.print_new_key(key);
        Ok(())
    }

    pub fn key(&self, key: &ApiKey) -> Result<()> {
        if self.json {
            return print_json(key);
        }

        print_fields(&key_fields(key));
        Ok(())
    }

    pub fn keys(&self, keys: &[ApiKey]) -> Result<()> {
        if self.json {
            return print_json(keys);
        }

        print_table(
            ["ID", "NAME", "PREFIX", "STATUS", "CREATED", "LAST USED"],
            keys.iter().map(|k| {
                [
                    k.id.to_string(),
                    k.name.clone(),
                    k.prefix.clone(),
                    key_status(k).to_owned(),
                    format_time(&k.created_at),
                    format_optional_time(k.last_used_at),
                ]
            }),
        );
        Ok(())
    }

    pub fn new_key(&self, key: &NewApiKey) -> Result<()> {
        if self.json {
            return print_json(&json!({ "key": key.key, "plaintext": key.plaintext }));
        }

        self.print_new_key(key);
        Ok(())
    }

    /// Prints a confirmation in text mode. JSON mode prints nothing, since the exit status says it succeeded.
    pub fn done(&self, message: &str) {
        if !self.json {
            println!("{message}");
        }
    }

    fn print_new_key(&self, key: &NewApiKey) {
        print_fields(&key_fields(&key.key));
        println!();
        println!("API key (shown only once, store it now):");
        println!("{}", key.plaintext);
    }
}

/// Asks a yes/no question on stderr. Anything but "y" or "yes" counts as no.
pub fn confirm(question: &str) -> Result<bool> {
    eprint!("{question} [y/N] ");
    io::stderr().flush()?;

    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;

    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn print_json(value: &(impl Serialize + ?Sized)) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn project_fields(project: &Project) -> Vec<(&'static str, String)> {
    vec![
        ("ID", project.id.to_string()),
        ("Slug", project.slug.clone()),
        ("Name", project.name.clone()),
        ("Created", format_time(&project.created_at)),
    ]
}

fn key_fields(key: &ApiKey) -> Vec<(&'static str, String)> {
    vec![
        ("ID", key.id.to_string()),
        ("Project", key.project_id.to_string()),
        ("Name", key.name.clone()),
        ("Prefix", key.prefix.clone()),
        ("Status", key_status(key).to_owned()),
        ("Created", format_time(&key.created_at)),
        ("Last used", format_optional_time(key.last_used_at)),
        ("Revoked", format_optional_time(key.revoked_at)),
    ]
}

fn format_time(time: &DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_optional_time(time: Option<DateTime<Utc>>) -> String {
    time.as_ref().map_or("-".to_owned(), format_time)
}

fn key_status(key: &ApiKey) -> &'static str {
    if key.revoked_at.is_some() {
        "revoked"
    } else {
        "active"
    }
}

fn print_fields(fields: &[(&str, String)]) {
    let width = fields
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    for (label, value) in fields {
        println!("{label:<width$}  {value}");
    }
}

fn print_table<const N: usize>(headers: [&str; N], rows: impl Iterator<Item = [String; N]>) {
    let rows: Vec<_> = rows.collect();
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }

    print_row(&headers, &widths);
    for row in &rows {
        print_row(row, &widths);
    }
}

fn print_row<const N: usize>(cells: &[impl AsRef<str>; N], widths: &[usize; N]) {
    let line: Vec<_> = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{:<width$}", cell.as_ref()))
        .collect();
    println!("{}", line.join("  ").trim_end());
}
