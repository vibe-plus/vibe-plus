//! Read-only SQLite extraction from `cc-switch.db` (CC Switch schema).

use super::types::{
    CcSwitchAppType, CcSwitchCustomEndpoint, CcSwitchProvider, CcSwitchProviderMeta,
    CcSwitchProxyConfig,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

pub struct CcSwitchDbReader {
    conn: Connection,
}

impl CcSwitchDbReader {
    pub fn open_read_only(db_path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("open cc-switch db: {}", db_path.display()))?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i32> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .context("read user_version")?;
        Ok(version)
    }

    pub fn load_db_settings(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key ASC")
            .context("prepare settings query")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("query settings")?;
        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row.context("settings row")?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn load_proxy_configs(&self) -> Result<Vec<CcSwitchProxyConfig>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT app_type, proxy_enabled, enabled, listen_address, listen_port
                 FROM proxy_config ORDER BY app_type ASC",
            )
            .context("prepare proxy_config query")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CcSwitchProxyConfig {
                    app_type: row.get(0)?,
                    proxy_enabled: row.get::<_, i32>(1)? != 0,
                    enabled: row.get::<_, i32>(2)? != 0,
                    listen_address: row.get(3)?,
                    listen_port: row.get::<_, i32>(4)? as u16,
                })
            })
            .context("query proxy_config")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("proxy_config row")?);
        }
        Ok(out)
    }

    pub fn load_providers_for_app(&self, app_type: &str) -> Result<Vec<CcSwitchProvider>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, settings_config, website_url, category, created_at, sort_index,
                        notes, icon, icon_color, meta, in_failover_queue, is_current
                 FROM providers WHERE app_type = ?1
                 ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC",
            )
            .context("prepare providers query")?;

        let rows = stmt
            .query_map([app_type], |row| {
                let id: String = row.get(0)?;
                let settings_config_str: String = row.get(2)?;
                let meta_str: String = row.get(10)?;
                let settings_config: serde_json::Value =
                    serde_json::from_str(&settings_config_str).unwrap_or(serde_json::Value::Null);
                let meta: CcSwitchProviderMeta =
                    serde_json::from_str(&meta_str).unwrap_or_default();

                Ok(CcSwitchProvider {
                    app_type: app_type.to_string(),
                    id,
                    name: row.get(1)?,
                    settings_config,
                    website_url: row.get(3)?,
                    category: row.get(4)?,
                    created_at: row.get(5)?,
                    sort_index: row.get(6)?,
                    notes: row.get(7)?,
                    icon: row.get(8)?,
                    icon_color: row.get(9)?,
                    meta: Some(meta),
                    in_failover_queue: row.get(11)?,
                    is_current_in_db: row.get::<_, i32>(12)? != 0,
                    custom_endpoints: Vec::new(),
                })
            })
            .with_context(|| format!("query providers for {app_type}"))?;

        let mut providers = Vec::new();
        for row in rows {
            let mut provider = row.with_context(|| format!("provider row ({app_type})"))?;
            provider.custom_endpoints = self.load_endpoints(&provider.id, app_type)?;
            providers.push(provider);
        }
        Ok(providers)
    }

    pub fn load_all_providers(&self) -> Result<Vec<CcSwitchProvider>> {
        let mut all = Vec::new();
        for app in CcSwitchAppType::ALL {
            all.extend(self.load_providers_for_app(app.as_str())?);
        }
        Ok(all)
    }

    pub fn current_provider_in_db(&self, app_type: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1")
            .context("prepare current provider query")?;
        let mut rows = stmt.query([app_type]).context("query current provider")?;
        match rows.next().context("current provider row")? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    fn load_endpoints(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<Vec<CcSwitchCustomEndpoint>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT url, added_at FROM provider_endpoints
                 WHERE provider_id = ?1 AND app_type = ?2
                 ORDER BY added_at ASC, url ASC",
            )
            .context("prepare provider_endpoints query")?;
        let rows = stmt
            .query_map([provider_id, app_type], |row| {
                Ok(CcSwitchCustomEndpoint {
                    url: row.get(0)?,
                    added_at: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    last_used: None,
                })
            })
            .context("query provider_endpoints")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("endpoint row")?);
        }
        Ok(out)
    }
}
