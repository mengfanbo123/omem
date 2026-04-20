use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::store::lancedb::SessionRecall;

const SHOULD_RECALL_SYSTEM_PROMPT: &str = r#"你是一个记忆召回助手。用户有一个个人知识库，保存了过往笔记、项目经验、技术方案、偏好设置等记忆。你的任务是判断用户当前的问题是否需要从知识库中检索相关记忆来辅助回答。如果是关于用户个人知识、项目细节、过往经验的问题，回答 yes。如果是通用常识、简单问候等无需检索的问题，回答 no。只回答 yes 或 no。"#;

#[derive(Deserialize)]
pub struct ShouldRecallRequest {
    pub query_text: String,
    pub last_query_text: Option<String>,
    pub session_id: String,
}

#[derive(Serialize)]
pub struct MemoryWithScore {
    pub memory: Memory,
    pub score: f32,
}

#[derive(Serialize)]
pub struct ShouldRecallResponse {
    pub should_recall: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryWithScore>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f32>,
}

#[derive(Deserialize)]
pub struct CreateSessionRecallRequest {
    pub session_id: String,
    pub memory_ids: Vec<String>,
    pub recall_type: String,
    #[serde(default)]
    pub query_text: String,
    #[serde(default)]
    pub similarity_score: f32,
    #[serde(default)]
    pub llm_confidence: f32,
}

#[derive(Deserialize)]
pub struct ListSessionRecallsQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub session_id: Option<String>,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
pub struct ListSessionRecallsResponse {
    pub recalls: Vec<SessionRecall>,
    pub limit: usize,
    pub offset: usize,
}

pub async fn should_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<ShouldRecallRequest>,
) -> Result<Json<ShouldRecallResponse>, OmemError> {
    if body.query_text.is_empty() {
        return Err(OmemError::Validation(
            "query_text cannot be empty".to_string(),
        ));
    }

    let similarity_score = if let Some(ref last_query) = body.last_query_text {
        if !last_query.is_empty() {
            let texts = vec![body.query_text.clone(), last_query.clone()];
            let embeddings = state
                .embed
                .embed(&texts)
                .await
                .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;

            if embeddings.len() == 2 {
                let sim = cosine_similarity(&embeddings[0], &embeddings[1]);
                if sim > 0.7 {
                    return Ok(Json(ShouldRecallResponse {
                        should_recall: false,
                        reason: Some("similarity_too_high".to_string()),
                        memories: None,
                        confidence: None,
                        similarity_score: Some(sim),
                    }));
                }
                Some(sim)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let system = SHOULD_RECALL_SYSTEM_PROMPT;
    let user = format!(
        "用户问题：{}\n\n这个问题是否需要从用户个人知识库中检索相关记忆来回答？回答 yes 或 no。",
        body.query_text
    );

    let _needs_recall = match state.recall_llm.complete_text(system, &user).await {
        Ok(llm_response) => {
            let needs = llm_response.trim().to_lowercase().starts_with("yes");
            if !needs {
                return Ok(Json(ShouldRecallResponse {
                    should_recall: false,
                    reason: Some("llm_decided_no".to_string()),
                    memories: None,
                    confidence: None,
                    similarity_score,
                }));
            }
            true
        }
        Err(_) => {
            true
        }
    };

    let vectors = state
        .embed
        .embed(std::slice::from_ref(&body.query_text))
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;
    let query_vector = vectors.into_iter().next();

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    const MIN_SCORE: f32 = 0.3;

    let is_zero_vector = query_vector.as_ref().map_or(true, |v| v.iter().all(|&x| x == 0.0));

    let results = if is_zero_vector {
        store
            .fts_search(&body.query_text, 5, None, None)
            .await
            .unwrap_or_default()
    } else {
        let search_vec = query_vector.unwrap();
        store
            .vector_search(&search_vec, 5, MIN_SCORE, None, None)
            .await
            .unwrap_or_default()
    };

    let memories: Vec<MemoryWithScore> = results
        .into_iter()
        .map(|(memory, score)| MemoryWithScore { memory, score })
        .collect();

    if memories.is_empty() {
        return Ok(Json(ShouldRecallResponse {
            should_recall: false,
            reason: Some("no_relevant_memories".to_string()),
            memories: None,
            confidence: None,
            similarity_score,
        }));
    }

    let confidence = memories.iter().map(|m| m.score).sum::<f32>() / memories.len() as f32;

    Ok(Json(ShouldRecallResponse {
        should_recall: true,
        reason: None,
        memories: Some(memories),
        confidence: Some(confidence),
        similarity_score,
    }))
}

pub async fn create_session_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreateSessionRecallRequest>,
) -> Result<Json<Vec<SessionRecall>>, OmemError> {
    if body.session_id.is_empty() {
        return Err(OmemError::Validation(
            "session_id cannot be empty".to_string(),
        ));
    }
    if body.memory_ids.is_empty() {
        return Err(OmemError::Validation(
            "memory_ids cannot be empty".to_string(),
        ));
    }
    if body.recall_type != "auto" && body.recall_type != "manual" {
        return Err(OmemError::Validation(
            "recall_type must be 'auto' or 'manual'".to_string(),
        ));
    }

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let mut recalls = Vec::new();
    for memory_id in body.memory_ids {
        let recall = SessionRecall {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: body.session_id.clone(),
            memory_id,
            recall_type: body.recall_type.clone(),
            query_text: body.query_text.clone(),
            similarity_score: body.similarity_score,
            llm_confidence: body.llm_confidence,
            tenant_id: auth.tenant_id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        store.create_session_recall(&recall).await?;
        recalls.push(recall);
    }

    Ok(Json(recalls))
}

pub async fn list_session_recalls(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<ListSessionRecallsQuery>,
) -> Result<Json<ListSessionRecallsResponse>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let recalls = store
        .list_session_recalls(
            &auth.tenant_id,
            params.session_id.as_deref(),
            params.limit,
            params.offset,
        )
        .await?;

    Ok(Json(ListSessionRecallsResponse {
        recalls,
        limit: params.limit,
        offset: params.offset,
    }))
}

pub async fn get_session_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<SessionRecall>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let recall = store
        .get_session_recall_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("session_recall {id}")))?;

    if recall.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("access denied".to_string()));
    }

    Ok(Json(recall))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
