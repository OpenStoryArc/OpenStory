//! Role-based identity for principals (Phase 6.3+6.4).
//!
//! A *principal* is the actor that authenticates against this OpenStory
//! node — a device, an automation agent, a CI runner. Every principal
//! belongs to exactly one *person* (Layer 2) and carries exactly one
//! *role* (Layer 3). The role determines what HTTP routes the principal
//! is permitted to invoke.
//!
//! ## Why a separate trait
//!
//! The `Directory` trait surfaced from PR #54 (in `rs/server/tests/directory/`)
//! handles personhood + group membership — who is a person, who's in
//! which group. Role checks need *one* lookup at request time —
//! `role_for_principal(principal_id)` — and that's what this module
//! exposes. Keeping the role surface tiny lets the auth middleware do its
//! one query without dragging in the rest of the personhood model.
//!
//! ## Ordering of roles
//!
//! `Observer < Contributor < Admin`. Routes declare a *minimum* role; the
//! middleware checks `principal_role >= required`. Adding a new role
//! tier (e.g. `Auditor`) means inserting it into the enum's ordering and
//! deciding which routes the new tier can call. No middleware changes.

use anyhow::Result;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// The role catalog. Total order ascending: `Observer` is the most-
/// restricted tier, `Admin` is the least.
///
/// Numeric values are NOT stable storage — the `to_str` / `FromStr` impls
/// are how roles round-trip through the SQLite participants table and any
/// future serialization. Re-numbering is fine; renaming is not.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Observer,
    Contributor,
    Admin,
}

impl Role {
    /// Canonical string representation for storage. Stable contract — used
    /// as the SQLite participants.role column value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Observer => "observer",
            Role::Contributor => "contributor",
            Role::Admin => "admin",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "observer" => Ok(Role::Observer),
            "contributor" => Ok(Role::Contributor),
            "admin" => Ok(Role::Admin),
            other => Err(format!(
                "unknown role '{other}': expected observer | contributor | admin"
            )),
        }
    }
}

/// One row in the participants table. Wraps the SQLite shape so the trait
/// returns typed values, not raw strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub principal_id: String,
    pub person_id: String,
    pub role: Role,
    /// ISO-8601 wall clock when this participant was upserted.
    pub created_at: String,
}

/// The role-lookup contract. Implementations: `EmbeddedRoleDirectory`
/// (SQLite, default), eventually a Keycloak-backed adapter, eventually
/// a NATS-publish/subscribe directory for federated deployments.
///
/// Methods are tiny on purpose. The auth middleware calls
/// `role_for_principal` on every authenticated request — adding heavy
/// methods to this trait would hurt the request hot path.
#[async_trait]
pub trait RoleDirectory: Send + Sync + 'static {
    async fn role_for_principal(&self, principal_id: &str) -> Result<Option<Role>>;
    async fn upsert_participant(&self, p: Participant) -> Result<()>;
    async fn lookup_participant(&self, principal_id: &str) -> Result<Option<Participant>>;
    async fn list_participants(&self) -> Result<Vec<Participant>>;
    async fn delete_participant(&self, principal_id: &str) -> Result<()>;
}

/// No-op directory for tests + bootstrap. `role_for_principal` always
/// returns `None`, so role-gated routes return 403 — fail-closed.
pub struct NoopRoleDirectory;

#[async_trait]
impl RoleDirectory for NoopRoleDirectory {
    async fn role_for_principal(&self, _principal_id: &str) -> Result<Option<Role>> {
        Ok(None)
    }
    async fn upsert_participant(&self, _p: Participant) -> Result<()> {
        Ok(())
    }
    async fn lookup_participant(&self, _principal_id: &str) -> Result<Option<Participant>> {
        Ok(None)
    }
    async fn list_participants(&self) -> Result<Vec<Participant>> {
        Ok(Vec::new())
    }
    async fn delete_participant(&self, _principal_id: &str) -> Result<()> {
        Ok(())
    }
}

/// SQLite-backed RoleDirectory. One file, one table, one row per
/// principal. Concurrency: a `Mutex<Connection>` is sufficient — role
/// lookups are rare (once per authenticated request) and the table is
/// small (principals per node measured in tens, not millions).
///
/// The table schema:
///
/// ```sql
/// CREATE TABLE participants (
///     principal_id TEXT PRIMARY KEY,
///     person_id    TEXT NOT NULL,
///     role         TEXT NOT NULL CHECK (role IN ('observer','contributor','admin')),
///     created_at   TEXT NOT NULL
/// );
/// ```
pub struct EmbeddedRoleDirectory {
    conn: Mutex<Connection>,
}

impl EmbeddedRoleDirectory {
    /// Open or create a directory at `path`. Calls `init_schema` so the
    /// participants table exists; safe to call against an existing file.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory variant for tests. Same schema, no on-disk footprint.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS participants (
                principal_id TEXT PRIMARY KEY,
                person_id    TEXT NOT NULL,
                role         TEXT NOT NULL CHECK (role IN ('observer','contributor','admin')),
                created_at   TEXT NOT NULL
            );",
        )?;
        Ok(())
    }
}

#[async_trait]
impl RoleDirectory for EmbeddedRoleDirectory {
    async fn role_for_principal(&self, principal_id: &str) -> Result<Option<Role>> {
        let conn = self.conn.lock().unwrap();
        let row: rusqlite::Result<String> = conn.query_row(
            "SELECT role FROM participants WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get(0),
        );
        match row {
            Ok(s) => Ok(Some(s.parse().map_err(anyhow::Error::msg)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn upsert_participant(&self, p: Participant) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO participants (principal_id, person_id, role, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(principal_id) DO UPDATE SET
                 person_id = excluded.person_id,
                 role = excluded.role,
                 created_at = excluded.created_at",
            params![p.principal_id, p.person_id, p.role.as_str(), p.created_at],
        )?;
        Ok(())
    }

    async fn lookup_participant(&self, principal_id: &str) -> Result<Option<Participant>> {
        let conn = self.conn.lock().unwrap();
        let row: rusqlite::Result<(String, String, String, String)> = conn.query_row(
            "SELECT principal_id, person_id, role, created_at FROM participants WHERE principal_id = ?1",
            params![principal_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        );
        match row {
            Ok((pid, pers, role, created_at)) => Ok(Some(Participant {
                principal_id: pid,
                person_id: pers,
                role: role.parse().map_err(anyhow::Error::msg)?,
                created_at,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn list_participants(&self) -> Result<Vec<Participant>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT principal_id, person_id, role, created_at FROM participants ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (pid, pers, role, created_at) = row?;
            out.push(Participant {
                principal_id: pid,
                person_id: pers,
                role: role.parse().map_err(anyhow::Error::msg)?,
                created_at,
            });
        }
        Ok(out)
    }

    async fn delete_participant(&self, principal_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM participants WHERE principal_id = ?1",
            params![principal_id],
        )?;
        Ok(())
    }
}

/// Keycloak-backed adapter stub. The hooks are present so production
/// configurations can opt in without an interface change; the methods
/// currently return `unimplemented` errors so anyone reaching for it
/// gets a clear signal. Wiring against the real Keycloak admin API
/// lives in a follow-up — the existing `rs/server/tests/directory/keycloak.rs`
/// test integration proves the connection pattern works.
pub struct KeycloakRoleDirectory {
    pub base_url: String,
    pub realm: String,
    pub admin_token: String,
}

#[async_trait]
impl RoleDirectory for KeycloakRoleDirectory {
    async fn role_for_principal(&self, _principal_id: &str) -> Result<Option<Role>> {
        anyhow::bail!(
            "KeycloakRoleDirectory not yet implemented — use EmbeddedRoleDirectory for now"
        )
    }
    async fn upsert_participant(&self, _p: Participant) -> Result<()> {
        anyhow::bail!("KeycloakRoleDirectory not yet implemented")
    }
    async fn lookup_participant(&self, _principal_id: &str) -> Result<Option<Participant>> {
        anyhow::bail!("KeycloakRoleDirectory not yet implemented")
    }
    async fn list_participants(&self) -> Result<Vec<Participant>> {
        anyhow::bail!("KeycloakRoleDirectory not yet implemented")
    }
    async fn delete_participant(&self, _principal_id: &str) -> Result<()> {
        anyhow::bail!("KeycloakRoleDirectory not yet implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(principal_id: &str, person_id: &str, role: Role) -> Participant {
        Participant {
            principal_id: principal_id.to_string(),
            person_id: person_id.to_string(),
            role,
            created_at: "2026-05-31T00:00:00Z".to_string(),
        }
    }

    // ── Role enum ───────────────────────────────────────────────────────

    #[test]
    fn role_total_order_observer_lowest_admin_highest() {
        assert!(Role::Observer < Role::Contributor);
        assert!(Role::Contributor < Role::Admin);
        assert!(Role::Observer < Role::Admin);
    }

    #[test]
    fn role_round_trips_through_as_str_and_from_str() {
        for r in [Role::Observer, Role::Contributor, Role::Admin] {
            assert_eq!(r.as_str().parse::<Role>().unwrap(), r);
        }
    }

    #[test]
    fn role_from_str_normalizes_case() {
        assert_eq!("OBSERVER".parse::<Role>().unwrap(), Role::Observer);
        assert_eq!("Admin".parse::<Role>().unwrap(), Role::Admin);
    }

    #[test]
    fn role_from_str_rejects_unknown() {
        let err = "wizard".parse::<Role>().unwrap_err();
        assert!(err.contains("unknown role"));
    }

    // ── EmbeddedRoleDirectory ───────────────────────────────────────────

    #[tokio::test]
    async fn role_for_unknown_principal_is_none() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        let role = dir.role_for_principal("never-seen").await.unwrap();
        assert!(role.is_none());
    }

    #[tokio::test]
    async fn upsert_then_lookup_returns_the_participant() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        dir.upsert_participant(p("max-laptop", "max", Role::Admin))
            .await
            .unwrap();

        let role = dir.role_for_principal("max-laptop").await.unwrap();
        assert_eq!(role, Some(Role::Admin));

        let participant = dir.lookup_participant("max-laptop").await.unwrap().unwrap();
        assert_eq!(participant.principal_id, "max-laptop");
        assert_eq!(participant.person_id, "max");
        assert_eq!(participant.role, Role::Admin);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_overwrites_role() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        dir.upsert_participant(p("max-laptop", "max", Role::Observer))
            .await
            .unwrap();
        dir.upsert_participant(p("max-laptop", "max", Role::Admin))
            .await
            .unwrap();

        let role = dir.role_for_principal("max-laptop").await.unwrap();
        assert_eq!(role, Some(Role::Admin), "second upsert should replace role");
        let all = dir.list_participants().await.unwrap();
        assert_eq!(all.len(), 1, "idempotent — still one row");
    }

    #[tokio::test]
    async fn delete_removes_the_participant() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        dir.upsert_participant(p("max-laptop", "max", Role::Admin))
            .await
            .unwrap();
        dir.delete_participant("max-laptop").await.unwrap();
        assert!(dir.role_for_principal("max-laptop").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_returns_all_participants_in_insert_order() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        let mut p1 = p("a", "max", Role::Observer);
        p1.created_at = "2026-05-01T00:00:00Z".into();
        let mut p2 = p("b", "max", Role::Contributor);
        p2.created_at = "2026-05-02T00:00:00Z".into();
        let mut p3 = p("c", "katie", Role::Admin);
        p3.created_at = "2026-05-03T00:00:00Z".into();
        dir.upsert_participant(p1).await.unwrap();
        dir.upsert_participant(p2).await.unwrap();
        dir.upsert_participant(p3).await.unwrap();

        let all = dir.list_participants().await.unwrap();
        assert_eq!(
            all.iter().map(|p| p.principal_id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[tokio::test]
    async fn delete_unknown_principal_is_a_noop() {
        let dir = EmbeddedRoleDirectory::in_memory().unwrap();
        // Should not error.
        dir.delete_participant("never-existed").await.unwrap();
    }

    #[tokio::test]
    async fn schema_persists_across_open() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("roles.db");
        {
            let dir = EmbeddedRoleDirectory::open(&path).unwrap();
            dir.upsert_participant(p("max-laptop", "max", Role::Contributor))
                .await
                .unwrap();
        }
        // Reopen — data and schema both survive.
        let dir = EmbeddedRoleDirectory::open(&path).unwrap();
        let role = dir.role_for_principal("max-laptop").await.unwrap();
        assert_eq!(role, Some(Role::Contributor));
    }

    // ── NoopRoleDirectory ───────────────────────────────────────────────

    #[tokio::test]
    async fn noop_returns_none_for_any_principal() {
        let dir = NoopRoleDirectory;
        assert!(dir.role_for_principal("max-laptop").await.unwrap().is_none());
        assert!(dir.role_for_principal("").await.unwrap().is_none());
    }
}
