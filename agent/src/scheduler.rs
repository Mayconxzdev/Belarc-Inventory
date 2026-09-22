use std::time::Duration;

use belarc_shared::{CollectionTier, InventoryPayload};
use chrono::Utc;

use crate::cache::LocalCache;
use crate::client::ServerClient;
use crate::collector::{
    build_heartbeat_from_identity, build_register_from_identity, filter_changed, run_collector,
    run_tier, update_cache_hashes,
};
use crate::config::{self, AgentConfig};

pub async fn run_agent_loop() -> Result<(), Box<dyn std::error::Error>> {
    let config = AgentConfig::load();
    if config.agent_token.is_empty() {
        return Err(
            "BELARC_AGENT_TOKEN is required. Create one via POST /api/tokens on the server.".into(),
        );
    }

    let cache_path = config::data_dir().join("cache.db");
    let cache = LocalCache::open(&cache_path)?;
    cache.migrate()?;

    let client = ServerClient::new(&config);

    // Initial registration
    if let Ok(identity) = run_collector("identity") {
        let reg = build_register_from_identity(&identity, &config.agent_token);
        if let Err(e) = client.register(reg).await {
            tracing::warn!("register failed (may already exist): {e}");
        }
        let hb = build_heartbeat_from_identity(&identity);
        if let Err(e) = client.heartbeat(hb).await {
            tracing::warn!("initial heartbeat failed: {e}");
        }
    }

    // T1 on startup
    run_and_sync(&config, &client, &cache, "t1").await?;

    let hb_secs = config.heartbeat_interval_seconds;
    let t1_secs = config.t1_interval_seconds;
    let t2_secs = config.t2_interval_seconds;

    let mut heartbeat_tick = tokio::time::interval(Duration::from_secs(hb_secs));
    let mut t1_tick = tokio::time::interval(Duration::from_secs(t1_secs));
    let mut t2_tick = tokio::time::interval(Duration::from_secs(t2_secs));

    // Stagger T2 to run after startup
    t2_tick.tick().await;
    {
        let config = config.clone();
        let client = ServerClient::new(&config);
        let cache_path = config::data_dir().join("cache.db");
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if let Ok(cache) = LocalCache::open(&cache_path) {
                let _ = run_and_sync(&config, &client, &cache, "t2").await;
            }
        });
    }

    loop {
        tokio::select! {
            _ = heartbeat_tick.tick() => {
                if let Ok(identity) = run_collector("identity") {
                    let hb = build_heartbeat_from_identity(&identity);
                    if let Err(e) = client.heartbeat(hb.clone()).await {
                        tracing::warn!("heartbeat failed: {e}");
                        let json = serde_json::to_string(&hb).unwrap_or_default();
                        let _ = cache.queue_upload("heartbeat", &json);
                    }
                }
                let _ = client.flush_pending(&cache).await;
            }
            _ = t1_tick.tick() => {
                if let Err(e) = run_and_sync(&config, &client, &cache, "t1").await {
                    tracing::warn!("t1 sync failed: {e}");
                }
            }
            _ = t2_tick.tick() => {
                if let Err(e) = run_and_sync(&config, &client, &cache, "t2").await {
                    tracing::warn!("t2 sync failed: {e}");
                }
            }
        }
    }
}

pub async fn run_collection_once(
    tier: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = AgentConfig::load();
    let cache_path = config::data_dir().join("cache.db");
    let cache = LocalCache::open(&cache_path)?;
    cache.migrate()?;
    if force {
        cache.clear_hashes()?;
        tracing::info!("cache cleared — full sync will be sent");
    }
    let client = ServerClient::new(&config);
    if let Ok(identity) = run_collector("identity") {
        let reg = build_register_from_identity(&identity, &config.agent_token);
        let _ = client.register(reg).await;
        let hb = build_heartbeat_from_identity(&identity);
        if let Err(e) = client.heartbeat(hb).await {
            tracing::warn!("heartbeat after collect failed: {e}");
        }
    }
    run_and_sync(&config, &client, &cache, tier).await?;
    let _ = client.flush_pending(&cache).await;
    Ok(())
}

async fn run_and_sync(
    config: &AgentConfig,
    client: &ServerClient,
    cache: &LocalCache,
    tier: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(identity) = run_collector("identity") {
        let reg = build_register_from_identity(&identity, &config.agent_token);
        let _ = client.register(reg).await;
        let hb = build_heartbeat_from_identity(&identity);
        let _ = client.heartbeat(hb).await;
    }

    let all_results = run_tier(tier);
    let changed = filter_changed(all_results, cache);

    if changed.is_empty() {
        tracing::debug!("tier {tier}: no changes");
        return Ok(());
    }

    let hostname = changed
        .iter()
        .find(|c| c.name == "identity")
        .and_then(|c| c.data.get("hostname"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let collection_tier = match tier {
        "t1" => CollectionTier::T1,
        "t2" => CollectionTier::T2,
        "t3" => CollectionTier::T3,
        _ => CollectionTier::T1,
    };

    let payload = InventoryPayload {
        agent_token: String::new(),
        hostname,
        tier: collection_tier,
        collectors: changed.clone(),
        collected_at: Utc::now(),
    };

    if let Err(e) = client.inventory(payload.clone()).await {
        tracing::warn!("inventory upload failed: {e}");
        let json = serde_json::to_string(&payload)?;
        cache.queue_upload("inventory", &json)?;
    } else {
        update_cache_hashes(&changed, cache);
        tracing::info!("tier {tier}: synced {} collectors", changed.len());
    }

    Ok(())
}
