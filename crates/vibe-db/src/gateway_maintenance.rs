//! Persistent records for deferred gateway maintenance tasks.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::dao::now_secs;
use crate::Db;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayMaintenanceTaskRecord {
    pub task_id: String,
    pub last_run_at: i64,
    pub last_version: Option<String>,
    pub last_input_stamp: Option<String>,
    pub last_result: Option<String>,
    pub run_count: i64,
}

impl Db {
    pub fn maintenance_task_get(
        &self,
        task_id: &str,
    ) -> Result<Option<GatewayMaintenanceTaskRecord>> {
        self.with(|c| maintenance_task_get_conn(c, task_id))
    }

    pub fn maintenance_task_record_run(
        &self,
        task_id: &str,
        version: Option<&str>,
        input_stamp: Option<&str>,
        result: Option<&str>,
    ) -> Result<()> {
        self.with(|c| maintenance_task_record_run_conn(c, task_id, version, input_stamp, result))
    }
}

fn maintenance_task_get_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<GatewayMaintenanceTaskRecord>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, last_run_at, last_version, last_input_stamp, last_result, run_count
         FROM gateway_maintenance_tasks
         WHERE task_id = ?1",
    )?;
    let mut rows = stmt.query(params![task_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(GatewayMaintenanceTaskRecord {
        task_id: row.get(0)?,
        last_run_at: row.get(1)?,
        last_version: row.get(2)?,
        last_input_stamp: row.get(3)?,
        last_result: row.get(4)?,
        run_count: row.get(5)?,
    }))
}

fn maintenance_task_record_run_conn(
    conn: &Connection,
    task_id: &str,
    version: Option<&str>,
    input_stamp: Option<&str>,
    result: Option<&str>,
) -> Result<()> {
    let now = now_secs();
    conn.execute(
        "INSERT INTO gateway_maintenance_tasks (
            task_id, last_run_at, last_version, last_input_stamp, last_result, run_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, 1)
         ON CONFLICT(task_id) DO UPDATE SET
            last_run_at = excluded.last_run_at,
            last_version = excluded.last_version,
            last_input_stamp = excluded.last_input_stamp,
            last_result = excluded.last_result,
            run_count = gateway_maintenance_tasks.run_count + 1",
        params![task_id, now, version, input_stamp, result],
    )?;
    Ok(())
}
