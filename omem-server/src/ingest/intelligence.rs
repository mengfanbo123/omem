use std::sync::Arc;

use tracing::{error, info, warn};

use crate::domain::error::OmemError;
use crate::embed::EmbedService;
use crate::ingest::extractor::FactExtractor;
use crate::ingest::reconciler::Reconciler;
use crate::ingest::session::SessionStore;
use crate::ingest::types::IngestMessage;
use crate::llm::LlmService;
use crate::store::{LanceStore, SpaceStore};

const SMART_SPLIT_MAX_CHARS: usize = 80_000;
const SMART_SPLIT_OVERLAP: usize = 2_000;

pub struct IntelligenceTask {
    session_store: Arc<SessionStore>,
    extractor: Arc<FactExtractor>,
    reconciler: Arc<Reconciler>,
    space_store: Arc<SpaceStore>,
    task_id: String,
    tenant_id: String,
}

impl IntelligenceTask {
    pub fn new(
        store: Arc<LanceStore>,
        session_store: Arc<SessionStore>,
        embed: Arc<dyn EmbedService>,
        llm: Arc<dyn LlmService>,
        space_store: Arc<SpaceStore>,
        task_id: String,
        tenant_id: String,
    ) -> Self {
        let mut extractor = FactExtractor::new(llm.clone());
        extractor.max_input_chars = SMART_SPLIT_MAX_CHARS;

        let reconciler = Reconciler::new(llm, store, embed);

        Self {
            session_store,
            extractor: Arc::new(extractor),
            reconciler: Arc::new(reconciler),
            space_store,
            task_id,
            tenant_id,
        }
    }

    pub async fn run(&self) {
        if let Err(e) = self.run_inner().await {
            error!(task_id = %self.task_id, error = %e, "intelligence task failed");
            self.set_task_field(|t| {
                t.status = "failed".to_string();
                t.errors.push(format!("{e}"));
                t.completed_at = Some(chrono::Utc::now().to_rfc3339());
            })
            .await;
        }
    }

    async fn run_inner(&self) -> Result<(), OmemError> {
        self.set_task_field(|t| t.extraction_status = "running".to_string())
            .await;

        let raw_messages = self
            .session_store
            .find_by_session_id(&format!("import-{}", self.task_id))
            .await?;

        if raw_messages.is_empty() {
            info!(task_id = %self.task_id, "no import session data found, completing");
            self.set_task_field(|t| {
                t.extraction_status = "completed".to_string();
                t.reconcile_status = "skipped".to_string();
                t.status = "completed".to_string();
                t.completed_at = Some(chrono::Utc::now().to_rfc3339());
            })
            .await;
            return Ok(());
        }

        let full_text = raw_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let chunks = smart_split(&full_text, SMART_SPLIT_MAX_CHARS, SMART_SPLIT_OVERLAP);

        let mut all_facts = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let messages = vec![IngestMessage {
                role: "user".to_string(),
                content: chunk.to_string(),
            }];

            match self.extractor.extract(&messages, None).await {
                Ok(facts) => {
                    all_facts.extend(facts);
                    let facts_count = all_facts.len();
                    let total = chunks.len();
                    let progress = i + 1;
                    self.set_task_field(move |t| {
                        t.extraction_chunks = total;
                        t.extraction_facts = facts_count;
                        t.extraction_progress = progress;
                    })
                    .await;
                }
                Err(e) => {
                    warn!(chunk = i, error = %e, task_id = %self.task_id, "chunk extraction failed");
                    let err_msg = format!("chunk {} extraction failed: {}", i, e);
                    self.set_task_field(move |t| t.errors.push(err_msg))
                        .await;
                }
            }
        }

        self.set_task_field(|t| t.extraction_status = "completed".to_string())
            .await;

        if all_facts.is_empty() {
            info!(task_id = %self.task_id, "no facts extracted, completing");
            self.set_task_field(|t| {
                t.reconcile_status = "skipped".to_string();
                t.status = "completed".to_string();
                t.completed_at = Some(chrono::Utc::now().to_rfc3339());
            })
            .await;
            return Ok(());
        }

        self.set_task_field(|t| t.reconcile_status = "running".to_string())
            .await;

        match self.reconciler.reconcile(&all_facts, &self.tenant_id).await {
            Ok(memories) => {
                let fact_count = all_facts.len();
                let mem_count = memories.len();
                info!(
                    task_id = %self.task_id,
                    fact_count,
                    memory_count = mem_count,
                    "intelligence reconciliation complete"
                );
                self.set_task_field(move |t| {
                    t.reconcile_relations = fact_count;
                    t.reconcile_merged = mem_count;
                    t.reconcile_progress = fact_count;
                })
                .await;
            }
            Err(e) => {
                error!(error = %e, task_id = %self.task_id, "reconciliation failed");
                let err_msg = format!("reconciliation failed: {e}");
                self.set_task_field(move |t| t.errors.push(err_msg))
                    .await;
            }
        }

        self.set_task_field(|t| {
            t.reconcile_status = "completed".to_string();
            t.status = "completed".to_string();
            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
        })
        .await;

        Ok(())
    }

    async fn set_task_field<F: FnOnce(&mut crate::store::spaces::ImportTaskRecord)>(&self, f: F) {
        if let Ok(Some(mut task)) = self.space_store.get_import_task(&self.task_id).await {
            f(&mut task);
            let _ = self.space_store.update_import_task(&task).await;
        }
    }
}

pub fn smart_split(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chars).min(text.len());
        let boundary = find_best_boundary(text, start, end);
        chunks.push(text[start..boundary].to_string());

        if boundary >= text.len() {
            break;
        }

        let next_start = if boundary > overlap {
            boundary - overlap
        } else {
            boundary
        };
        if next_start <= start {
            start = boundary;
        } else {
            start = next_start;
        }
    }
    chunks
}

fn find_best_boundary(text: &str, start: usize, end: usize) -> usize {
    if end >= text.len() {
        return text.len();
    }
    let window = &text[start..end];
    if let Some(pos) = window.rfind("\n## ") {
        return start + pos + 1;
    }
    if let Some(pos) = window.rfind("\n\n") {
        return start + pos + 2;
    }
    if let Some(pos) = window.rfind('\n') {
        return start + pos + 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk_for_small_text() {
        let chunks = smart_split("hello world", 100, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hello world");
    }

    #[test]
    fn splits_at_double_newline() {
        let text = "part one\n\npart two\n\npart three";
        let chunks = smart_split(text, 15, 0);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].ends_with("\n\n") || !chunks[0].contains("part two"));
    }

    #[test]
    fn handles_no_boundary() {
        let text = "a".repeat(200);
        let chunks = smart_split(&text, 100, 0);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 100);
        assert_eq!(chunks[1].len(), 100);
    }

    #[test]
    fn empty_text() {
        let chunks = smart_split("", 100, 0);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn exact_boundary() {
        let text = "abc\n\ndef\n\nghi";
        let chunks = smart_split(text, 5, 0);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(!chunk.is_empty());
        }
    }

    #[test]
    fn overlap_produces_shared_content() {
        let text = "aaaa\n\nbbbb\n\ncccc\n\ndddd";
        let chunks = smart_split(text, 12, 6);
        assert!(chunks.len() >= 2);
        if chunks.len() >= 2 {
            let first_end = &chunks[0][chunks[0].len().saturating_sub(6)..];
            assert!(
                chunks[1].starts_with(first_end) || chunks[1].contains(&chunks[0][chunks[0].len().saturating_sub(4)..]),
                "overlap content should appear in next chunk"
            );
        }
    }

    #[test]
    fn heading_boundary_preferred() {
        let text = format!("intro text\n\nparagraph\n## Heading\nmore text{}", "x".repeat(100));
        let chunks = smart_split(&text, 35, 0);
        assert!(chunks.len() >= 2);
        assert!(
            chunks[0].ends_with('\n') || !chunks[0].contains("## Heading"),
            "should split before heading"
        );
    }

    #[test]
    fn find_best_boundary_prefers_heading() {
        let text = "aaa\n\nbbb\n## heading\nccc";
        let b = find_best_boundary(text, 0, text.len() - 1);
        assert_eq!(&text[b..b + 2], "##");
    }

    #[test]
    fn find_best_boundary_falls_back_to_paragraph() {
        let text = "aaa\n\nbbb\nccc";
        let b = find_best_boundary(text, 0, text.len() - 1);
        assert_eq!(b, 5);
    }

    #[test]
    fn find_best_boundary_falls_back_to_newline() {
        let text = "aaa\nbbb";
        let b = find_best_boundary(text, 0, text.len() - 1);
        assert_eq!(b, 4);
    }

    #[test]
    fn find_best_boundary_hard_cut() {
        let text = "abcdefgh";
        let b = find_best_boundary(text, 0, 5);
        assert_eq!(b, 5);
    }
}
