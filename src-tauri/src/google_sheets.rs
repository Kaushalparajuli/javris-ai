//! Google Sheets: make spreadsheets, read ranges, and add or change cells.

use crate::google::{api, url_with, with_scope_hint};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

const SHEETS: &str = "https://sheets.googleapis.com/v4/spreadsheets";
/// Most rows and columns handed back from one read.
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 30;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetInfo {
    id: String,
    title: String,
    link: String,
    /// Tab names, in order.
    tabs: Vec<String>,
}

fn info_from(v: &Value) -> SheetInfo {
    let id = v["spreadsheetId"].as_str().unwrap_or("").to_string();
    SheetInfo {
        link: format!("https://docs.google.com/spreadsheets/d/{id}/edit"),
        title: v["properties"]["title"].as_str().unwrap_or("").into(),
        tabs: v["sheets"].as_array().map(|a| a.iter().filter_map(|s| s["properties"]["title"].as_str().map(String::from)).collect()).unwrap_or_default(),
        id,
    }
}

fn id_ok(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
        Ok(id)
    } else {
        Err("That isn't a Google Sheets id.".into())
    }
}

/// `…/spreadsheets/{id}/values/{range}{suffix}`, with the range (like `Sheet 1!A1:C9`) encoded.
fn values_url(id: &str, range: &str, suffix: &str, query: &[(&str, &str)]) -> String {
    let mut url = tauri::Url::parse(SHEETS).expect("valid base url");
    {
        let mut seg = url.path_segments_mut().expect("has a path");
        seg.push(id).push("values").push(&format!("{range}{suffix}"));
    }
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query.iter().copied());
    }
    url.to_string()
}

#[tauri::command]
pub async fn sheet_info(app: AppHandle, id: String) -> Result<SheetInfo, String> {
    let id = id_ok(&id)?;
    let url = url_with(&format!("{SHEETS}/{id}"), &[("fields", "spreadsheetId,properties.title,sheets.properties.title")]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    Ok(info_from(&v))
}

/// A new spreadsheet. `rows` (optional) fill the first tab from A1, usually a header row first.
#[tauri::command]
pub async fn sheet_create(app: AppHandle, title: String, tabs: Option<Vec<String>>, rows: Option<Vec<Vec<Value>>>) -> Result<SheetInfo, String> {
    let title = if title.trim().is_empty() { "Untitled".to_string() } else { title.trim().to_string() };
    let sheets: Vec<Value> = tabs.unwrap_or_default().into_iter().filter(|t| !t.trim().is_empty()).map(|t| json!({ "properties": { "title": t.trim() } })).collect();
    let mut body = json!({ "properties": { "title": title } });
    if !sheets.is_empty() {
        body["sheets"] = json!(sheets);
    }
    let v = api(&app, reqwest::Method::POST, SHEETS, Some(body)).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    let info = info_from(&v);
    if let Some(rows) = rows.filter(|r| !r.is_empty()) {
        let tab = info.tabs.first().cloned().unwrap_or_else(|| "Sheet1".into());
        let url = values_url(&info.id, &format!("{tab}!A1"), "", &[("valueInputOption", "USER_ENTERED")]);
        api(&app, reqwest::Method::PUT, &url, Some(json!({ "values": rows }))).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    }
    Ok(info)
}

#[derive(Serialize)]
pub struct Cells {
    range: String,
    values: Vec<Vec<String>>,
    truncated: bool,
}

/// Cell values as the user sees them. Cut to a size Jarvis can speak about.
#[tauri::command]
pub async fn sheet_read(app: AppHandle, id: String, range: String) -> Result<Cells, String> {
    let id = id_ok(&id)?;
    let url = values_url(id, &range, "", &[("valueRenderOption", "FORMATTED_VALUE")]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    Ok(cells_from(&v))
}

fn cells_from(v: &Value) -> Cells {
    let rows = v["values"].as_array().cloned().unwrap_or_default();
    let truncated = rows.len() > MAX_ROWS || rows.iter().any(|r| r.as_array().is_some_and(|c| c.len() > MAX_COLS));
    let values = rows
        .iter()
        .take(MAX_ROWS)
        .map(|r| r.as_array().map(|c| c.iter().take(MAX_COLS).map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default())
        .collect();
    Cells { range: v["range"].as_str().unwrap_or("").into(), values, truncated }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    range: String,
    rows: u64,
    cells: u64,
}

/// Add rows after the last row of data in the range's table.
#[tauri::command]
pub async fn sheet_append(app: AppHandle, id: String, range: String, rows: Vec<Vec<Value>>) -> Result<Written, String> {
    let id = id_ok(&id)?;
    if rows.is_empty() {
        return Err("No rows to add.".into());
    }
    let url = values_url(id, &range, ":append", &[("valueInputOption", "USER_ENTERED"), ("insertDataOption", "INSERT_ROWS")]);
    let v = api(&app, reqwest::Method::POST, &url, Some(json!({ "values": rows }))).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    let u = &v["updates"];
    Ok(Written { range: u["updatedRange"].as_str().unwrap_or("").into(), rows: u["updatedRows"].as_u64().unwrap_or(0), cells: u["updatedCells"].as_u64().unwrap_or(0) })
}

/// Overwrite the cells of a range with new values.
#[tauri::command]
pub async fn sheet_update(app: AppHandle, id: String, range: String, values: Vec<Vec<Value>>) -> Result<Written, String> {
    let id = id_ok(&id)?;
    let url = values_url(id, &range, "", &[("valueInputOption", "USER_ENTERED")]);
    let v = api(&app, reqwest::Method::PUT, &url, Some(json!({ "values": values }))).await.map_err(|e| with_scope_hint(e, "Sheets"))?;
    Ok(Written { range: v["updatedRange"].as_str().unwrap_or("").into(), rows: v["updatedRows"].as_u64().unwrap_or(0), cells: v["updatedCells"].as_u64().unwrap_or(0) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_is_encoded_in_the_path() {
        let u = values_url("abc", "Q3 Plan!A1:C9", "", &[("valueInputOption", "RAW")]);
        assert_eq!(u, "https://sheets.googleapis.com/v4/spreadsheets/abc/values/Q3%20Plan!A1:C9?valueInputOption=RAW");
        let a = values_url("abc", "Sheet1!A:C", ":append", &[]);
        assert!(a.ends_with("/values/Sheet1!A:C:append"), "{a}");
        // A hostile range can't climb out of the values path.
        assert!(!values_url("abc", "../../x?y#z", "", &[]).contains("/../"));
    }

    #[test]
    fn reads_are_capped() {
        let rows: Vec<Value> = (0..250).map(|i| json!([i.to_string(), 5, true])).collect();
        let c = cells_from(&json!({ "range": "S!A1", "values": rows }));
        assert_eq!(c.values.len(), MAX_ROWS);
        assert!(c.truncated);
        assert_eq!(c.values[0], vec!["0", "5", "true"]);
    }

    #[test]
    fn empty_range_reads_as_no_rows() {
        let c = cells_from(&json!({ "range": "S!A1:B2" }));
        assert!(c.values.is_empty() && !c.truncated);
    }
}
