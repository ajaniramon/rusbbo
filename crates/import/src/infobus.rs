//! `infobus_polls` and `infobus_polls_answers` (Java: `InfobusDao`).

use anyhow::{Context, Result};
use mysql::Row;
use mysql::prelude::Queryable;
use rusbbo_storage::{InfobusPoll, Store};

use crate::col;

/// Returns the number of rows copied.
pub fn import_infobus(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let polls: Vec<Row> = conn.query(
        "SELECT id, initiated_by, poll_data, \
         CAST(UNIX_TIMESTAMP(created_at) AS SIGNED) AS created_ts FROM infobus_polls ORDER BY id",
    )?;
    let answers: Vec<Row> =
        conn.query("SELECT poll_id, answer, user_id FROM infobus_polls_answers")?;
    let mut w = store.write()?;
    for row in &polls {
        let id: u32 = col(row, "id")?;
        let data: String = col(row, "poll_data")?;
        // Java: `InfobusPollData` ({"question": .., "answers": [..]}) as Gson writes it.
        let data: serde_json::Value = serde_json::from_str(&data)
            .with_context(|| format!("infobus poll {id}: bad poll_data"))?;
        let question = data["question"].as_str().unwrap_or_default().to_string();
        let answers = data["answers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let created_at: Option<i64> = col(row, "created_ts")?;
        w.put_infobus_poll(&InfobusPoll {
            id,
            initiated_by: col(row, "initiated_by")?,
            question,
            answers,
            created_at: created_at.unwrap_or(0),
        })?;
    }
    for row in &answers {
        w.put_infobus_answer(
            col(row, "poll_id")?,
            col(row, "answer")?,
            col(row, "user_id")?,
        )?;
    }
    if let Some(next) = crate::auto_increment(conn, "infobus_polls")? {
        w.set_auto_increment("infobus_polls", next as u32)?;
    }
    w.commit()?;
    Ok(polls.len() + answers.len())
}
