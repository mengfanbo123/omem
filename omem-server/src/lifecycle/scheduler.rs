use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::lifecycle::forgetting::AutoForgetter;
use crate::lifecycle::tier::TierManager;
use crate::store::StoreManager;

pub struct LifecycleScheduler {
    store_manager: Arc<StoreManager>,
    interval: Duration,
    max_memories_per_store: usize,
}

impl LifecycleScheduler {
    pub fn new(store_manager: Arc<StoreManager>, interval: Duration) -> Self {
        Self {
            store_manager,
            interval,
            max_memories_per_store: 5000,
        }
    }

    pub async fn run(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "lifecycle_scheduler_run_failed");
            }
        }
    }

    async fn run_once(&self) -> Result<(), crate::domain::error::OmemError> {
        let tier_manager = TierManager::with_defaults();
        let stores = self.store_manager.cached_stores().await;

        if stores.is_empty() {
            return Ok(());
        }

        for store in &stores {
            self.evaluate_tiers(store, &tier_manager).await;
            self.run_forgetting(store).await;
        }

        Ok(())
    }

    async fn evaluate_tiers(
        &self,
        store: &Arc<crate::store::LanceStore>,
        tier_manager: &TierManager,
    ) {
        let memories = match store.list(self.max_memories_per_store, 0).await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "scheduler_failed_to_list_memories");
                return;
            }
        };

        let mut demoted_count = 0usize;
        for mut memory in memories {
            if memory.state != crate::domain::types::MemoryState::Active {
                continue;
            }

            let old_tier = memory.tier.clone();
            let new_tier = tier_manager.evaluate_tier(&memory);

            if new_tier != old_tier {
                tracing::info!(
                    memory_id = %memory.id,
                    old_tier = %old_tier,
                    new_tier = %new_tier,
                    access_count = memory.access_count,
                    "tier_changed_by_scheduler"
                );
                memory.tier = new_tier;
                if let Err(e) = store.update(&memory, None).await {
                    warn!(memory_id = %memory.id, error = %e, "scheduler_failed_to_update_tier");
                }
                demoted_count += 1;
            }
        }

        if demoted_count > 0 {
            info!(demoted = demoted_count, "scheduler_tier_evaluation_complete");
        }
    }

    async fn run_forgetting(&self, store: &Arc<crate::store::LanceStore>) {
        let forgetter = AutoForgetter::new(store.clone());

        match forgetter.cleanup_expired().await {
            Ok(count) if count > 0 => {
                info!(expired = count, "scheduler_expired_cleanup");
            }
            Err(e) => {
                warn!(error = %e, "scheduler_cleanup_expired_failed");
            }
            _ => {}
        }

        match forgetter.archive_superseded(30).await {
            Ok(count) if count > 0 => {
                info!(archived = count, "scheduler_superseded_archive");
            }
            Err(e) => {
                warn!(error = %e, "scheduler_archive_superseded_failed");
            }
            _ => {}
        }
    }
}
