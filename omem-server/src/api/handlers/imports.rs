use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum_extra::extract::Multipart;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::domain::types::MemoryType;

#[derive(Serialize, Clone)]
pub struct ImportTask {
    pub id: String,
    pub status: String,
    pub file_type: String,
    pub filename: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub space_id: String,
    pub total_items: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Deserialize)]
pub struct ListImportsQuery {
    #[serde(default = "default_import_limit")]
    pub limit: usize,
}

fn default_import_limit() -> usize { 50 }

pub async fn create_import(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    mut multipart: Multipart,
) -> Result<Json<ImportTask>, OmemError> {
    let mut file_data: Option<Vec<u8>> = None;
    let mut filename = String::new();
    let mut file_type = String::from("memory");
    let mut agent_id: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut space_id: Option<String> = None;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| OmemError::Validation(format!("multipart error: {e}")))? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                filename = field.file_name().unwrap_or("unknown").to_string();
                file_data = Some(field.bytes().await
                    .map_err(|e| OmemError::Validation(format!("read file: {e}")))?.to_vec());
            }
            "file_type" => { file_type = field.text().await.map_err(|e| OmemError::Validation(format!("{e}")))?; }
            "agent_id" => { agent_id = Some(field.text().await.map_err(|e| OmemError::Validation(format!("{e}")))?); }
            "session_id" => { session_id = Some(field.text().await.map_err(|e| OmemError::Validation(format!("{e}")))?); }
            "space_id" => { space_id = Some(field.text().await.map_err(|e| OmemError::Validation(format!("{e}")))?); }
            _ => {}
        }
    }

    let data = file_data.ok_or_else(|| OmemError::Validation("no 'file' field".to_string()))?;
    let content = String::from_utf8(data).map_err(|_| OmemError::Validation("not valid UTF-8".to_string()))?;
    let target_space = space_id.unwrap_or_else(|| personal_space_id(&auth.tenant_id));
    let store = state.store_manager.get_store(&target_space).await?;
    let task_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let total_items;

    match file_type.as_str() {
        "memory" => {
            let items = parse_memory_json(&content)?;
            total_items = items.len();
            for item in &items {
                let c = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if c.is_empty() { skipped += 1; continue; }
                let tags: Vec<String> = item.get("tags").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_else(|| vec!["imported".into()]);
                let mut m = Memory::new(c, crate::domain::category::Category::Entities, MemoryType::Insight, &auth.tenant_id);
                m.tags = tags; m.source = Some("import".into()); m.space_id = target_space.clone();
                if let Some(ref a) = agent_id { m.owner_agent_id = a.clone(); }
                let v = embed(&state, c).await;
                match store.create(&m, v.as_deref()).await { Ok(()) => imported += 1, Err(e) => errors.push(format!("{e}")) }
            }
        }
        "session" => {
            let msgs = parse_session(&content);
            total_items = msgs.len();
            for (role, c) in &msgs {
                if c.len() < 20 { skipped += 1; continue; }
                let cat = if role == "user" { crate::domain::category::Category::Events } else { crate::domain::category::Category::Cases };
                let mut m = Memory::new(c, cat, MemoryType::Session, &auth.tenant_id);
                m.source = Some("import-session".into()); m.space_id = target_space.clone();
                if let Some(ref s) = session_id { m.session_id = Some(s.clone()); }
                if let Some(ref a) = agent_id { m.owner_agent_id = a.clone(); }
                let v = embed(&state, c).await;
                match store.create(&m, v.as_deref()).await { Ok(()) => imported += 1, Err(e) => errors.push(format!("{e}")) }
            }
        }
        "markdown" => {
            let paras: Vec<&str> = content.split("\n\n").map(|p| p.trim()).filter(|p| p.len() > 20).collect();
            total_items = paras.len();
            for &p in &paras {
                let mut m = Memory::new(p, crate::domain::category::Category::Entities, MemoryType::Insight, &auth.tenant_id);
                m.tags = vec!["imported".into(), "markdown".into()]; m.source = Some("import-markdown".into()); m.space_id = target_space.clone();
                if let Some(ref a) = agent_id { m.owner_agent_id = a.clone(); }
                let v = embed(&state, p).await;
                match store.create(&m, v.as_deref()).await { Ok(()) => imported += 1, Err(e) => errors.push(format!("{e}")) }
            }
        }
        "jsonl" => {
            let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            total_items = lines.len();
            for line in &lines {
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                    let c = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if c.is_empty() { skipped += 1; continue; }
                    let mut m = Memory::new(c, crate::domain::category::Category::Entities, MemoryType::Insight, &auth.tenant_id);
                    m.source = Some("import-jsonl".into()); m.space_id = target_space.clone();
                    if let Some(ref a) = agent_id { m.owner_agent_id = a.clone(); }
                    let v = embed(&state, c).await;
                    match store.create(&m, v.as_deref()).await { Ok(()) => imported += 1, Err(e) => errors.push(format!("{e}")) }
                } else { skipped += 1; }
            }
        }
        _ => return Err(OmemError::Validation(format!("unsupported file_type: {file_type}. Use: memory, session, markdown, jsonl"))),
    }

    let status = if errors.is_empty() { "completed" } else if imported > 0 { "partial" } else { "failed" };
    Ok(Json(ImportTask {
        id: task_id, status: status.into(), file_type, filename, agent_id, session_id,
        space_id: target_space, total_items, imported, skipped, errors,
        created_at: now, completed_at: Some(chrono::Utc::now().to_rfc3339()),
    }))
}

pub async fn list_imports(
    State(_state): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthInfo>,
    Query(_params): Query<ListImportsQuery>,
) -> Result<Json<serde_json::Value>, OmemError> {
    Ok(Json(serde_json::json!({"imports": [], "total": 0})))
}

pub async fn get_import(
    State(_state): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    Err(OmemError::NotFound(format!("import task {id}")))
}

async fn embed(state: &AppState, text: &str) -> Option<Vec<f32>> {
    state.embed.embed(&[text.to_string()]).await.ok().and_then(|v| v.into_iter().next())
}

fn parse_memory_json(content: &str) -> Result<Vec<serde_json::Value>, OmemError> {
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(content) { return Ok(arr); }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(mems) = obj.get("memories").and_then(|m| m.as_array()) { return Ok(mems.clone()); }
        return Ok(vec![obj]);
    }
    Err(OmemError::Validation("expected JSON array or object with 'memories' field".into()))
}

fn parse_session(content: &str) -> Vec<(String, String)> {
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(content) {
        return arr.iter().filter_map(|m| {
            Some((m.get("role")?.as_str()?.into(), m.get("content")?.as_str()?.into()))
        }).collect();
    }
    content.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter_map(|m| {
        Some((m.get("role")?.as_str()?.into(), m.get("content")?.as_str()?.into()))
    }).collect()
}
