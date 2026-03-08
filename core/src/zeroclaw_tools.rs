use crate::{oauth_store, settings::KaizenSettings};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const GMAIL_API_BASE_URL: &str = "https://gmail.googleapis.com/gmail/v1";
const DEFAULT_REPORT_FORMAT: &str = "xlsx";
const DEFAULT_REPORT_EXPORT_DIR: &str = "data/worker_artifacts";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeToolStatus {
    pub id: String,
    pub label: String,
    pub category: String,
    pub available: bool,
    pub connected: bool,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailToolConfig {
    pub enabled: bool,
    pub supported: bool,
    pub connected: bool,
    pub access_token_configured: bool,
    pub refresh_token_configured: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportsToolConfig {
    pub ready: bool,
    pub export_dir: String,
    pub default_format: String,
    pub formats: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZeroclawToolConfigResponse {
    pub gmail: GmailToolConfig,
    pub reports: ReportsToolConfig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConnectResponse {
    pub tool_id: String,
    pub started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolRunRequest {
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunResponse {
    pub tool_id: String,
    pub action: String,
    pub ok: bool,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailMessageSummary {
    pub id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportExportResult {
    pub row_count: usize,
    pub columns: Vec<String>,
    pub artifact_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailComposeRequest {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

pub async fn collect_native_tool_statuses(
    settings: &KaizenSettings,
    workspace_root: &Path,
) -> Vec<NativeToolStatus> {
    let gmail = gmail_tool_status(settings).await;
    let reports = reports_tool_status(settings, workspace_root);
    vec![gmail, reports]
}

pub async fn collect_tool_config(
    settings: &KaizenSettings,
    workspace_root: &Path,
) -> ZeroclawToolConfigResponse {
    ZeroclawToolConfigResponse {
        gmail: gmail_tool_config(settings).await,
        reports: reports_tool_config(settings, workspace_root),
    }
}

pub async fn connect_tool(tool_id: &str, bind_port: u16) -> Result<ToolConnectResponse, String> {
    match tool_id.trim().to_ascii_lowercase().as_str() {
        "gmail" => {
            let (_pending, redirect_url) =
                oauth_store::start_gmail_oauth(default_gmail_oauth_redirect_uri(bind_port))?;
            Ok(ToolConnectResponse {
                tool_id: "gmail".to_string(),
                started: true,
                redirect_url: Some(redirect_url),
                message: "Gmail OAuth opened in your browser.".to_string(),
            })
        }
        "reports" | "sheets" => Ok(ToolConnectResponse {
            tool_id: "reports".to_string(),
            started: false,
            redirect_url: None,
            message: "Reports do not require an external account connection.".to_string(),
        }),
        other => Err(format!("Unknown Zeroclaw tool '{}'.", other)),
    }
}

pub async fn run_tool(
    settings: &KaizenSettings,
    workspace_root: &Path,
    tool_id: &str,
    request: ToolRunRequest,
) -> Result<ToolRunResponse, String> {
    match tool_id.trim().to_ascii_lowercase().as_str() {
        "gmail" => run_gmail_tool(settings, request).await,
        "reports" | "sheets" => run_reports_tool(settings, workspace_root, request),
        other => Err(format!("Unknown Zeroclaw tool '{}'.", other)),
    }
}

pub fn report_export_dir(settings: &KaizenSettings, workspace_root: &Path) -> PathBuf {
    let configured = settings.zeroclaw_report_export_dir.trim();
    if configured.is_empty() {
        return workspace_root.join(DEFAULT_REPORT_EXPORT_DIR);
    }

    let candidate = PathBuf::from(configured);
    if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    }
}

pub fn report_default_format(settings: &KaizenSettings) -> String {
    let value = settings
        .zeroclaw_report_default_format
        .trim()
        .to_ascii_lowercase();
    if matches!(value.as_str(), "csv" | "xlsx") {
        value
    } else {
        DEFAULT_REPORT_FORMAT.to_string()
    }
}

pub async fn gmail_tool_config(settings: &KaizenSettings) -> GmailToolConfig {
    if !settings.zeroclaw_gmail_enabled {
        return GmailToolConfig {
            enabled: false,
            supported: true,
            connected: false,
            access_token_configured: false,
            refresh_token_configured: false,
            message: "Gmail is disabled in Kaizen settings.".to_string(),
        };
    }

    match oauth_store::stored_gmail_oauth_status() {
        Ok(status) => GmailToolConfig {
            enabled: true,
            supported: true,
            connected: status.connected(),
            access_token_configured: status.access_token_present,
            refresh_token_configured: status.refresh_token_present,
            message: status.message,
        },
        Err(error) => GmailToolConfig {
            enabled: true,
            supported: true,
            connected: false,
            access_token_configured: false,
            refresh_token_configured: false,
            message: format!("Gmail OAuth state could not be read: {}", error),
        },
    }
}

pub async fn gmail_tool_status(settings: &KaizenSettings) -> NativeToolStatus {
    let config = gmail_tool_config(settings).await;
    NativeToolStatus {
        id: "gmail".to_string(),
        label: "Gmail".to_string(),
        category: "business".to_string(),
        available: config.enabled,
        connected: config.connected,
        status: if !config.enabled {
            "disabled".to_string()
        } else if config.connected {
            "ready".to_string()
        } else {
            "needs_setup".to_string()
        },
        message: config.message,
    }
}

pub fn reports_tool_config(settings: &KaizenSettings, workspace_root: &Path) -> ReportsToolConfig {
    let export_dir = report_export_dir(settings, workspace_root);
    ReportsToolConfig {
        ready: true,
        export_dir: export_dir.display().to_string(),
        default_format: report_default_format(settings),
        formats: vec!["csv".to_string(), "xlsx".to_string()],
        message: "Reports can be exported as CSV and XLSX artifacts inside the Kaizen workspace."
            .to_string(),
    }
}

pub fn reports_tool_status(settings: &KaizenSettings, workspace_root: &Path) -> NativeToolStatus {
    let config = reports_tool_config(settings, workspace_root);
    NativeToolStatus {
        id: "reports".to_string(),
        label: "Reports".to_string(),
        category: "business".to_string(),
        available: true,
        connected: config.ready,
        status: "ready".to_string(),
        message: format!(
            "Exports {} reports to {}.",
            config.default_format.to_uppercase(),
            config.export_dir
        ),
    }
}
pub async fn gmail_search_messages(
    query: &str,
    limit: usize,
) -> Result<Vec<GmailMessageSummary>, String> {
    let token = gmail_access_token().await?;
    let base = gmail_api_base_url();
    let client = reqwest::Client::builder()
        .user_agent("kaizen-gateway/0.1.0")
        .build()
        .map_err(|error| format!("Failed to build Gmail HTTP client: {error}"))?;

    let list_url = format!("{}/users/me/messages", base);
    let list_response = client
        .get(list_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .query(&[("q", query), ("maxResults", &limit.to_string())])
        .send()
        .await
        .map_err(|error| format!("Gmail search request failed: {error}"))?;

    let list_value = parse_google_json(list_response).await?;
    let message_refs = list_value
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for message_ref in message_refs.into_iter().take(limit) {
        let Some(message_id) = message_ref.get("id").and_then(Value::as_str) else {
            continue;
        };
        let detail_url = format!("{}/users/me/messages/{}", base, message_id);
        let detail_response = client
            .get(detail_url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .query(&[
                ("format", "metadata"),
                ("metadataHeaders", "Subject"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "To"),
                ("metadataHeaders", "Date"),
            ])
            .send()
            .await
            .map_err(|error| format!("Gmail message fetch failed: {error}"))?;
        let detail = parse_google_json(detail_response).await?;
        out.push(GmailMessageSummary {
            id: message_id.to_string(),
            thread_id: detail
                .get("threadId")
                .and_then(Value::as_str)
                .map(|value| value.to_string()),
            snippet: detail
                .get("snippet")
                .and_then(Value::as_str)
                .map(|value| value.to_string()),
            subject: gmail_header(&detail, "Subject"),
            from: gmail_header(&detail, "From"),
            to: gmail_header(&detail, "To"),
            date: gmail_header(&detail, "Date"),
        });
    }

    Ok(out)
}

pub async fn gmail_create_draft(request: &GmailComposeRequest) -> Result<String, String> {
    gmail_submit_message(request, true).await
}

pub async fn gmail_send_message(request: &GmailComposeRequest) -> Result<String, String> {
    gmail_submit_message(request, false).await
}

pub fn export_report_artifacts(
    settings: &KaizenSettings,
    workspace_root: &Path,
    file_stem: &str,
    rows: &[Map<String, Value>],
) -> Result<ReportExportResult, String> {
    let export_dir = report_export_dir(settings, workspace_root);
    fs::create_dir_all(&export_dir).map_err(|error| {
        format!(
            "Failed to create report export directory '{}': {error}",
            export_dir.display()
        )
    })?;

    let stem = sanitize_file_stem(file_stem);
    let csv_path = export_dir.join(format!("{}.csv", stem));
    let xlsx_path = export_dir.join(format!("{}.xlsx", stem));
    let columns = collect_columns(rows);

    write_csv_report(&csv_path, &columns, rows)?;
    write_xlsx_report(&xlsx_path, &columns, rows)?;

    Ok(ReportExportResult {
        row_count: rows.len(),
        columns,
        artifact_paths: vec![
            csv_path.display().to_string(),
            xlsx_path.display().to_string(),
        ],
    })
}

fn run_reports_tool(
    settings: &KaizenSettings,
    workspace_root: &Path,
    request: ToolRunRequest,
) -> Result<ToolRunResponse, String> {
    let action = request.action.trim().to_ascii_lowercase();
    if action != "export" {
        return Err(format!("Reports action '{}' is not supported.", action));
    }

    let rows = request
        .args
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "Reports export needs args.rows as an array of objects.".to_string())?
        .iter()
        .map(value_to_row)
        .collect::<Result<Vec<_>, _>>()?;

    let stem = request
        .args
        .get("file_stem")
        .and_then(Value::as_str)
        .unwrap_or("zeroclaw-report");

    let result = export_report_artifacts(settings, workspace_root, stem, &rows)?;
    Ok(ToolRunResponse {
        tool_id: "reports".to_string(),
        action: "export".to_string(),
        ok: true,
        status: "completed".to_string(),
        message: format!(
            "Exported {} row(s) to {} artifact(s).",
            result.row_count,
            result.artifact_paths.len()
        ),
        artifact_paths: result.artifact_paths.clone(),
        data: serde_json::to_value(result).unwrap_or(Value::Null),
    })
}

async fn run_gmail_tool(
    settings: &KaizenSettings,
    request: ToolRunRequest,
) -> Result<ToolRunResponse, String> {
    if !settings.zeroclaw_gmail_enabled {
        return Ok(ToolRunResponse {
            tool_id: "gmail".to_string(),
            action: request.action,
            ok: false,
            status: "blocked".to_string(),
            message: "Gmail is disabled in Kaizen settings.".to_string(),
            artifact_paths: vec![],
            data: Value::Null,
        });
    }

    let action = request.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "status" => {
            let config = gmail_tool_config(settings).await;
            Ok(ToolRunResponse {
                tool_id: "gmail".to_string(),
                action,
                ok: config.connected,
                status: if config.connected {
                    "ready".to_string()
                } else {
                    "needs_setup".to_string()
                },
                message: config.message.clone(),
                artifact_paths: vec![],
                data: serde_json::to_value(config).unwrap_or(Value::Null),
            })
        }
        "search" | "list" => {
            let query = request
                .args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("in:inbox newer_than:14d");
            let limit = request
                .args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10) as usize;
            let messages = gmail_search_messages(query, limit.min(25)).await?;
            Ok(ToolRunResponse {
                tool_id: "gmail".to_string(),
                action,
                ok: true,
                status: "completed".to_string(),
                message: format!("Found {} Gmail message(s).", messages.len()),
                artifact_paths: vec![],
                data: serde_json::to_value(messages).unwrap_or(Value::Null),
            })
        }
        "draft" | "send" => {
            let compose = parse_gmail_compose_request(&request.args)?;
            let id = if action == "draft" {
                gmail_create_draft(&compose).await?
            } else {
                gmail_send_message(&compose).await?
            };
            Ok(ToolRunResponse {
                tool_id: "gmail".to_string(),
                action: action.clone(),
                ok: true,
                status: "completed".to_string(),
                message: if action == "draft" {
                    format!("Created Gmail draft {}.", id)
                } else {
                    format!("Sent Gmail message {}.", id)
                },
                artifact_paths: vec![],
                data: json!({ "id": id, "recipients": compose.to }),
            })
        }
        _ => Err(format!("Gmail action '{}' is not supported.", action)),
    }
}

fn default_gmail_oauth_redirect_uri(bind_port: u16) -> String {
    format!("http://127.0.0.1:{}/api/oauth/gmail/callback", bind_port)
}

fn gmail_api_base_url() -> String {
    std::env::var("KAIZEN_GMAIL_API_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| GMAIL_API_BASE_URL.to_string())
}

fn gmail_header(detail: &Value, header_name: &str) -> Option<String> {
    detail
        .get("payload")
        .and_then(|payload| payload.get("headers"))
        .and_then(Value::as_array)
        .and_then(|headers| {
            headers.iter().find_map(|header| {
                let name = header.get("name")?.as_str()?;
                if name.eq_ignore_ascii_case(header_name) {
                    header.get("value")?.as_str().map(|value| value.to_string())
                } else {
                    None
                }
            })
        })
}
fn parse_gmail_compose_request(value: &Value) -> Result<GmailComposeRequest, String> {
    let to = string_list_field(value, "to")?;
    if to.is_empty() {
        return Err("Gmail compose request needs at least one recipient in args.to.".to_string());
    }

    let subject = value
        .get("subject")
        .and_then(Value::as_str)
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .ok_or_else(|| "Gmail compose request needs args.subject.".to_string())?;

    let body = value
        .get("body")
        .and_then(Value::as_str)
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .ok_or_else(|| "Gmail compose request needs args.body.".to_string())?;

    Ok(GmailComposeRequest {
        to,
        cc: string_list_field(value, "cc")?,
        bcc: string_list_field(value, "bcc")?,
        subject,
        body,
    })
}

fn string_list_field(value: &Value, key: &str) -> Result<Vec<String>, String> {
    match value.get(key) {
        None => Ok(Vec::new()),
        Some(Value::String(single)) => Ok(single
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|item| {
                item.as_str()
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| format!("Field '{}' must only contain strings.", key))
            })
            .collect(),
        _ => Err(format!(
            "Field '{}' must be a string or array of strings.",
            key
        )),
    }
}

async fn gmail_submit_message(
    request: &GmailComposeRequest,
    as_draft: bool,
) -> Result<String, String> {
    let token = gmail_access_token().await?;
    let base = gmail_api_base_url();
    let client = reqwest::Client::builder()
        .user_agent("kaizen-gateway/0.1.0")
        .build()
        .map_err(|error| format!("Failed to build Gmail HTTP client: {error}"))?;

    let raw = URL_SAFE_NO_PAD.encode(build_rfc822_message(request).as_bytes());
    let url = if as_draft {
        format!("{}/users/me/drafts", base)
    } else {
        format!("{}/users/me/messages/send", base)
    };
    let body = if as_draft {
        json!({ "message": { "raw": raw } })
    } else {
        json!({ "raw": raw })
    };

    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            format!(
                "Gmail {} request failed: {error}",
                if as_draft { "draft" } else { "send" }
            )
        })?;

    let value = parse_google_json(response).await?;
    value
        .get("id")
        .or_else(|| value.get("message").and_then(|inner| inner.get("id")))
        .and_then(Value::as_str)
        .map(|item| item.to_string())
        .ok_or_else(|| "Gmail API succeeded, but no message id was returned.".to_string())
}

async fn gmail_access_token() -> Result<String, String> {
    let tokens = oauth_store::load_or_refresh_gmail_tokens()
        .await?
        .ok_or_else(|| {
            "Gmail is not connected. Connect Gmail in Integrations first.".to_string()
        })?;

    if tokens.access_token.trim().is_empty() {
        Err("Stored Gmail OAuth session does not have a usable access token.".to_string())
    } else {
        Ok(tokens.access_token)
    }
}

async fn parse_google_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed to read Google API response body: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "Google API request failed ({}): {}",
            status,
            if body.trim().is_empty() {
                "empty response body".to_string()
            } else {
                body
            }
        ));
    }

    serde_json::from_str(&body).map_err(|error| format!("Failed to parse Google API JSON: {error}"))
}

fn build_rfc822_message(request: &GmailComposeRequest) -> String {
    let mut lines = Vec::new();
    lines.push(format!("To: {}", request.to.join(", ")));
    if !request.cc.is_empty() {
        lines.push(format!("Cc: {}", request.cc.join(", ")));
    }
    if !request.bcc.is_empty() {
        lines.push(format!("Bcc: {}", request.bcc.join(", ")));
    }
    lines.push(format!("Subject: {}", request.subject));
    lines.push("MIME-Version: 1.0".to_string());
    lines.push("Content-Type: text/plain; charset=\"UTF-8\"".to_string());
    lines.push(String::new());
    lines.push(request.body.replace("\r\n", "\n"));
    lines.join("\r\n")
}

fn value_to_row(value: &Value) -> Result<Map<String, Value>, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "Each report row must be an object.".to_string())
}

fn collect_columns(rows: &[Map<String, Value>]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for row in rows {
        for key in row.keys() {
            set.insert(key.clone());
        }
    }
    set.into_iter().collect()
}

fn cell_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::Null) | None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn write_csv_report(
    path: &Path,
    columns: &[String],
    rows: &[Map<String, Value>],
) -> Result<(), String> {
    let mut out = String::new();
    out.push_str(
        &columns
            .iter()
            .map(|item| csv_escape(item))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in rows {
        let line = columns
            .iter()
            .map(|column| csv_escape(&cell_to_string(row.get(column))))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }
    fs::write(path, out)
        .map_err(|error| format!("Failed to write CSV report '{}': {error}", path.display()))
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
fn write_xlsx_report(
    path: &Path,
    columns: &[String],
    rows: &[Map<String, Value>],
) -> Result<(), String> {
    let file = fs::File::create(path)
        .map_err(|error| format!("Failed to create XLSX report '{}': {error}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options)
        .map_err(|error| format!("Failed to start XLSX content types: {error}"))?;
    zip.write_all(content_types_xml().as_bytes())
        .map_err(|error| format!("Failed to write XLSX content types: {error}"))?;

    zip.add_directory("_rels/", options)
        .map_err(|error| format!("Failed to add XLSX rels directory: {error}"))?;
    zip.start_file("_rels/.rels", options)
        .map_err(|error| format!("Failed to start XLSX rels: {error}"))?;
    zip.write_all(root_rels_xml().as_bytes())
        .map_err(|error| format!("Failed to write XLSX rels: {error}"))?;

    zip.add_directory("xl/", options)
        .map_err(|error| format!("Failed to add XLSX xl directory: {error}"))?;
    zip.start_file("xl/workbook.xml", options)
        .map_err(|error| format!("Failed to start XLSX workbook: {error}"))?;
    zip.write_all(workbook_xml().as_bytes())
        .map_err(|error| format!("Failed to write XLSX workbook: {error}"))?;

    zip.add_directory("xl/_rels/", options)
        .map_err(|error| format!("Failed to add XLSX workbook rels directory: {error}"))?;
    zip.start_file("xl/_rels/workbook.xml.rels", options)
        .map_err(|error| format!("Failed to start XLSX workbook rels: {error}"))?;
    zip.write_all(workbook_rels_xml().as_bytes())
        .map_err(|error| format!("Failed to write XLSX workbook rels: {error}"))?;

    zip.add_directory("xl/worksheets/", options)
        .map_err(|error| format!("Failed to add XLSX worksheet directory: {error}"))?;
    zip.start_file("xl/worksheets/sheet1.xml", options)
        .map_err(|error| format!("Failed to start XLSX worksheet: {error}"))?;
    zip.write_all(sheet_xml(columns, rows).as_bytes())
        .map_err(|error| format!("Failed to write XLSX worksheet: {error}"))?;

    zip.start_file("xl/styles.xml", options)
        .map_err(|error| format!("Failed to start XLSX styles: {error}"))?;
    zip.write_all(styles_xml().as_bytes())
        .map_err(|error| format!("Failed to write XLSX styles: {error}"))?;

    zip.finish().map_err(|error| {
        format!(
            "Failed to finalize XLSX report '{}': {error}",
            path.display()
        )
    })?;
    Ok(())
}

fn content_types_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>"#
}

fn root_rels_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#
}

fn workbook_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Report" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#
}

fn workbook_rels_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#
}

fn styles_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <fonts count="1"><font><sz val="11"/><name val="Aptos"/></font></fonts>
  <fills count="1"><fill><patternFill patternType="none"/></fill></fills>
  <borders count="1"><border/></borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>
</styleSheet>"#
}

fn sheet_xml(columns: &[String], rows: &[Map<String, Value>]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    xml.push_str(&sheet_row_xml(
        1,
        columns
            .iter()
            .map(|column| column.as_str())
            .collect::<Vec<_>>(),
    ));
    for (idx, row) in rows.iter().enumerate() {
        let values = columns
            .iter()
            .map(|column| cell_to_string(row.get(column)))
            .collect::<Vec<_>>();
        let refs = values
            .iter()
            .map(|value| value.as_str())
            .collect::<Vec<_>>();
        xml.push_str(&sheet_row_xml((idx + 2) as u32, refs));
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

fn sheet_row_xml(row_number: u32, values: Vec<&str>) -> String {
    let mut row = format!("<row r=\"{}\">", row_number);
    for (idx, value) in values.into_iter().enumerate() {
        let cell_ref = format!("{}{}", excel_column_name(idx), row_number);
        row.push_str(&format!(
            "<c r=\"{}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            cell_ref,
            xml_escape(value)
        ));
    }
    row.push_str("</row>");
    row
}

fn excel_column_name(index: usize) -> String {
    let mut n = index + 1;
    let mut out = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    out
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sanitize_file_stem(value: &str) -> String {
    let cleaned = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if cleaned.is_empty() {
        format!("report-{}", unix_epoch_secs())
    } else {
        cleaned
    }
}

fn unix_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::KaizenSettings;
    use tempfile::TempDir;

    #[test]
    fn report_export_writes_csv_and_xlsx() {
        let temp = TempDir::new().expect("temp dir");
        let mut settings = KaizenSettings::default();
        settings.zeroclaw_report_export_dir = temp.path().display().to_string();
        let rows = vec![
            Map::from_iter(vec![
                ("name".to_string(), Value::String("Ada".to_string())),
                (
                    "email".to_string(),
                    Value::String("ada@example.com".to_string()),
                ),
            ]),
            Map::from_iter(vec![
                ("name".to_string(), Value::String("Linus".to_string())),
                (
                    "email".to_string(),
                    Value::String("linus@example.com".to_string()),
                ),
            ]),
        ];

        let result =
            export_report_artifacts(&settings, temp.path(), "prospects", &rows).expect("export");
        assert_eq!(result.row_count, 2);
        assert_eq!(result.artifact_paths.len(), 2);
        assert!(Path::new(&result.artifact_paths[0]).exists());
        assert!(Path::new(&result.artifact_paths[1]).exists());
    }
}
