//! Hotel bans, IP logs and the moderation audit log (Java: `BanDao`, the `users_ip_logs` part of
//! `PlayerDao`, `ModerationDao`).
//!
//! Layout, from the `WHERE` clauses of those DAOs:
//! - `users_bans`: `row id -> Ban`, with `users_bans_by_value: (ban type, banned value) -> row
//!   ids` for `BanDao.hasBan`/`removeBan`. The MariaDB table has no primary key; the row id is
//!   the insertion order.
//! - `users_ip_logs`: `user_id -> Vec<IpLog>` in insertion order (`getLatestIp` takes the
//!   newest `created_at`).
//! - `housekeeping_audit_log`: `row id -> AuditLogEntry`, append only.
//! - `rank_badges`: `(rank, badge) -> ()` (Java: `BadgeDao.getRankBadges`, read by `:givebadge`).

use redb::{
    MultimapTableDefinition, ReadableMultimapTable, ReadableTable, TableDefinition,
    WriteTransaction,
};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const BANS: TableDefinition<u64, &[u8]> = TableDefinition::new("users_bans");
const BANS_BY_VALUE: MultimapTableDefinition<(u8, &str), u64> =
    MultimapTableDefinition::new("users_bans_by_value");
const IP_LOGS: TableDefinition<u32, &[u8]> = TableDefinition::new("users_ip_logs");
const AUDIT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("housekeeping_audit_log");
const RANK_BADGES: TableDefinition<(u8, &str), ()> = TableDefinition::new("rank_badges");

/// Java: `BanType`, the `ban_type` enum column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BanType {
    UserId,
    MachineId,
    IpAddress,
}

impl BanType {
    /// The column value (Java: `BanType.name()`).
    pub fn name(self) -> &'static str {
        match self {
            BanType::UserId => "USER_ID",
            BanType::MachineId => "MACHINE_ID",
            BanType::IpAddress => "IP_ADDRESS",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [BanType::UserId, BanType::MachineId, BanType::IpAddress]
            .into_iter()
            .find(|t| t.name() == name)
    }

    fn key(self) -> u8 {
        match self {
            BanType::UserId => 0,
            BanType::MachineId => 1,
            BanType::IpAddress => 2,
        }
    }
}

/// A row of `users_bans`. Times are Unix seconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    pub ban_type: BanType,
    pub banned_value: String,
    pub message: String,
    pub banned_until: i64,
    pub banned_at: i64,
    /// The banning user's id; `-1` for the word filter's automatic ban, `0` for legacy rows.
    pub banned_by: i32,
    pub is_active: bool,
}

/// A row of `users_ip_logs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpLog {
    pub ip_address: String,
    pub created_at: i64,
}

/// A row of `housekeeping_audit_log`. `action` is the enum column's text (`alert_user`...).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub action: String,
    pub user_id: i32,
    pub target_id: i32,
    pub message: String,
    pub extra_notes: String,
    pub created_at: i64,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(BANS)?;
    tx.open_multimap_table(BANS_BY_VALUE)?;
    tx.open_table(IP_LOGS)?;
    tx.open_table(AUDIT_LOG)?;
    tx.open_table(RANK_BADGES)?;
    Ok(())
}

impl Store {
    /// Java: `BanDao.hasBan`: the active ban of this type and value that ends last, if it ends
    /// after `now`.
    pub fn active_ban(&self, ban_type: BanType, value: &str, now: i64) -> Result<Option<Ban>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_multimap_table(BANS_BY_VALUE)?;
        let bans = tx.open_table(BANS)?;
        let mut best: Option<Ban> = None;
        for id in index.get((ban_type.key(), value))? {
            let Some(bytes) = bans.get(id?.value())? else {
                continue;
            };
            let ban: Ban = decode(bytes.value())?;
            if ban.is_active
                && ban.banned_until > now
                && best
                    .as_ref()
                    .is_none_or(|b| ban.banned_until > b.banned_until)
            {
                best = Some(ban);
            }
        }
        Ok(best)
    }

    /// Java: `BanDao.addBan`: nothing is stored for a blank value.
    pub fn add_ban(&self, ban: &Ban) -> Result<()> {
        let mut w = self.write()?;
        w.put_ban(ban)?;
        w.commit()
    }

    /// Java: `BanDao.removeBan`: every ban of this type and value becomes inactive. Returns how
    /// many rows changed.
    pub fn remove_ban(&self, ban_type: BanType, value: &str) -> Result<usize> {
        let w = self.write()?;
        let mut changed = 0;
        {
            let index = w.tx.open_multimap_table(BANS_BY_VALUE)?;
            let mut bans = w.tx.open_table(BANS)?;
            let ids: Vec<u64> = index
                .get((ban_type.key(), value))?
                .map(|id| id.map(|v| v.value()))
                .collect::<std::result::Result<_, _>>()?;
            for id in ids {
                let ban: Option<Ban> = bans.get(id)?.map(|b| decode(b.value())).transpose()?;
                if let Some(mut ban) = ban {
                    if ban.is_active {
                        ban.is_active = false;
                        bans.insert(id, encode(&ban)?.as_slice())?;
                        changed += 1;
                    }
                }
            }
        }
        w.commit()?;
        Ok(changed)
    }

    /// Every ban row, in insertion order (housekeeping and tests).
    pub fn bans(&self) -> Result<Vec<Ban>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(BANS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `PlayerDao.getLatestIp`.
    pub fn latest_ip(&self, user_id: u32) -> Result<Option<String>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(IP_LOGS)?;
        let Some(bytes) = table.get(user_id)? else {
            return Ok(None);
        };
        let logs: Vec<IpLog> = decode(bytes.value())?;
        // ORDER BY created_at DESC LIMIT 1; of rows in the same second the last one written.
        Ok(logs
            .iter()
            .rev()
            .max_by_key(|l| l.created_at)
            .map(|l| l.ip_address.clone()))
    }

    /// Java: `PlayerDao.logIpAddress`.
    pub fn log_ip(&self, user_id: u32, ip_address: &str, now: i64) -> Result<()> {
        let mut w = self.write()?;
        w.put_ip_log(
            user_id,
            IpLog {
                ip_address: ip_address.to_string(),
                created_at: now,
            },
        )?;
        w.commit()
    }

    /// Java: `ModerationDao.addLog`.
    pub fn add_audit_log(&self, entry: &AuditLogEntry) -> Result<()> {
        let mut w = self.write()?;
        w.put_audit_log(entry)?;
        w.commit()
    }

    /// Java: `BadgeDao.getRankBadges`: the badges that come with a rank.
    pub fn rank_badges(&self) -> Result<Vec<String>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(RANK_BADGES)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(entry?.0.value().1.to_string());
        }
        Ok(out)
    }

    /// Every audit log row, oldest first.
    pub fn audit_log(&self) -> Result<Vec<AuditLogEntry>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(AUDIT_LOG)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }
}

fn next_id(table: &redb::Table<u64, &[u8]>) -> Result<u64> {
    Ok(table.last()?.map_or(1, |(k, _)| k.value() + 1))
}

impl Writer {
    /// Imports or adds one `users_bans` row (Java: `addBan` skips a blank value).
    pub fn put_ban(&mut self, ban: &Ban) -> Result<()> {
        if ban.banned_value.trim().is_empty() {
            return Ok(());
        }
        let mut bans = self.tx.open_table(BANS)?;
        let id = next_id(&bans)?;
        bans.insert(id, encode(ban)?.as_slice())?;
        self.tx
            .open_multimap_table(BANS_BY_VALUE)?
            .insert((ban.ban_type.key(), ban.banned_value.as_str()), id)?;
        Ok(())
    }

    /// Imports or adds one `users_ip_logs` row.
    pub fn put_ip_log(&mut self, user_id: u32, log: IpLog) -> Result<()> {
        let mut table = self.tx.open_table(IP_LOGS)?;
        let mut logs: Vec<IpLog> = table
            .get(user_id)?
            .map(|b| decode(b.value()))
            .transpose()?
            .unwrap_or_default();
        logs.push(log);
        table.insert(user_id, encode(&logs)?.as_slice())?;
        Ok(())
    }

    /// Imports one `rank_badges` row.
    pub fn put_rank_badge(&mut self, rank: u8, badge: &str) -> Result<()> {
        self.tx.open_table(RANK_BADGES)?.insert((rank, badge), ())?;
        Ok(())
    }

    /// Imports or adds one `housekeeping_audit_log` row.
    pub fn put_audit_log(&mut self, entry: &AuditLogEntry) -> Result<()> {
        let mut table = self.tx.open_table(AUDIT_LOG)?;
        let id = next_id(&table)?;
        table.insert(id, encode(entry)?.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ban(t: BanType, value: &str, until: i64, active: bool) -> Ban {
        Ban {
            ban_type: t,
            banned_value: value.into(),
            message: format!("until {until}"),
            banned_until: until,
            banned_at: 0,
            banned_by: 1,
            is_active: active,
        }
    }

    #[test]
    fn bans_like_ban_dao() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        store
            .add_ban(&ban(BanType::UserId, "7", 100, true))
            .unwrap();
        store
            .add_ban(&ban(BanType::UserId, "7", 300, true))
            .unwrap();
        store
            .add_ban(&ban(BanType::UserId, "7", 900, false))
            .unwrap();
        store
            .add_ban(&ban(BanType::MachineId, "7", 500, true))
            .unwrap();
        store
            .add_ban(&ban(BanType::UserId, " ", 500, true))
            .unwrap();
        assert_eq!(store.bans().unwrap().len(), 4);

        // The latest active ban that has not ended; inactive rows never count.
        let found = store.active_ban(BanType::UserId, "7", 50).unwrap().unwrap();
        assert_eq!(found.banned_until, 300);
        assert!(
            store
                .active_ban(BanType::UserId, "7", 300)
                .unwrap()
                .is_none()
        );
        assert!(store.active_ban(BanType::UserId, "8", 0).unwrap().is_none());

        assert_eq!(store.remove_ban(BanType::UserId, "7").unwrap(), 2);
        assert!(
            store
                .active_ban(BanType::UserId, "7", 50)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .active_ban(BanType::MachineId, "7", 50)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn ip_logs_and_audit_log() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        assert_eq!(store.latest_ip(1).unwrap(), None);
        store.log_ip(1, "10.0.0.1", 5).unwrap();
        store.log_ip(1, "10.0.0.2", 9).unwrap();
        store.log_ip(1, "10.0.0.3", 7).unwrap();
        assert_eq!(store.latest_ip(1).unwrap().as_deref(), Some("10.0.0.2"));
        let entry = AuditLogEntry {
            action: "room_alert".into(),
            user_id: -1,
            target_id: 3,
            message: "hi".into(),
            extra_notes: String::new(),
            created_at: 1,
        };
        store.add_audit_log(&entry).unwrap();
        store.add_audit_log(&entry).unwrap();
        assert_eq!(store.audit_log().unwrap(), vec![entry.clone(), entry]);
    }
}
