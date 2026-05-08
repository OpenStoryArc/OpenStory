//! Directory pluggability spike — trait + shared types.
//!
//! See `docs/research/personhood-and-principals.md` for the philosophy.
//! See `directory_pluggability.rs` (sibling) for the test entry.
//!
//! The trait is intentionally tiny: five methods, opaque IDs. Anything more
//! belongs above (OpenStory-specific roles, policies, retention) or below
//! (impl-specific persistence, transport).

use anyhow::Result;
use async_trait::async_trait;

pub mod conformance;
pub mod embedded;
pub mod keycloak;

/// Opaque handle for a person within a directory. The shape is
/// implementation-defined (UUID for embedded, Keycloak's user UUID for the
/// IdP backend). Callers don't construct these — they receive them from
/// `upsert_person` and pass them back.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersonId(pub String);

/// Opaque handle for a group. Caller-chosen string for the spike (the impl
/// is responsible for any internal mapping — e.g. Keycloak's group paths).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPerson {
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: PersonId,
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub display_name: String,
}

/// The pluggable directory trait. Five methods.
///
/// Contract notes:
/// - `upsert_person` is idempotent on `email` (re-upserting the same email
///   returns the existing `PersonId`).
/// - `add_to_group` is idempotent (adding a person already in the group is
///   not an error).
/// - `add_to_group` materializes the group on first add — no separate
///   `create_group` call is needed for the spike.
/// - `members_of_group` for an unknown group returns an empty `Vec`, not an
///   error. (The "unknown" case is "no one is in this group yet".)
/// - Order of returned `Vec`s is not guaranteed. Callers (and conformance
///   assertions) must use set semantics.
#[async_trait]
pub trait Directory: Send + Sync {
    async fn upsert_person(&self, person: NewPerson) -> Result<PersonId>;
    async fn lookup_person(&self, id: &PersonId) -> Result<Option<Person>>;
    async fn add_to_group(&self, person: &PersonId, group: &GroupId) -> Result<()>;
    async fn groups_for_person(&self, person: &PersonId) -> Result<Vec<Group>>;
    async fn members_of_group(&self, group: &GroupId) -> Result<Vec<Person>>;
}
