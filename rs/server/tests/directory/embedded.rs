//! In-memory embedded `Directory` impl for the spike.
//!
//! HashMap-backed, single mutex around the whole state for trivial
//! atomicity. The persistence story (SQLite in the data dir) is deferred
//! until the spike validates the trait shape — what we're proving here is
//! that the trait is implementable and conforms, not that it persists.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::*;

#[derive(Default)]
struct State {
    persons_by_id: HashMap<PersonId, Person>,
    persons_by_email: HashMap<String, PersonId>,
    groups: HashMap<GroupId, Group>,
    memberships: HashMap<GroupId, HashSet<PersonId>>,
}

#[derive(Default)]
pub struct EmbeddedDirectory {
    state: Mutex<State>,
}

impl EmbeddedDirectory {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Directory for EmbeddedDirectory {
    async fn upsert_person(&self, person: NewPerson) -> Result<PersonId> {
        let mut state = self.state.lock().expect("state mutex poisoned");
        if let Some(existing) = state.persons_by_email.get(&person.email) {
            return Ok(existing.clone());
        }
        let id = PersonId(Uuid::new_v4().to_string());
        state.persons_by_id.insert(
            id.clone(),
            Person {
                id: id.clone(),
                display_name: person.display_name.clone(),
                email: person.email.clone(),
            },
        );
        state.persons_by_email.insert(person.email, id.clone());
        Ok(id)
    }

    async fn lookup_person(&self, id: &PersonId) -> Result<Option<Person>> {
        let state = self.state.lock().expect("state mutex poisoned");
        Ok(state.persons_by_id.get(id).cloned())
    }

    async fn add_to_group(&self, person: &PersonId, group: &GroupId) -> Result<()> {
        let mut state = self.state.lock().expect("state mutex poisoned");
        state.groups.entry(group.clone()).or_insert_with(|| Group {
            id: group.clone(),
            display_name: group.0.clone(),
        });
        state
            .memberships
            .entry(group.clone())
            .or_default()
            .insert(person.clone());
        Ok(())
    }

    async fn groups_for_person(&self, person: &PersonId) -> Result<Vec<Group>> {
        let state = self.state.lock().expect("state mutex poisoned");
        let groups: Vec<Group> = state
            .memberships
            .iter()
            .filter(|(_, members)| members.contains(person))
            .filter_map(|(gid, _)| state.groups.get(gid).cloned())
            .collect();
        Ok(groups)
    }

    async fn members_of_group(&self, group: &GroupId) -> Result<Vec<Person>> {
        let state = self.state.lock().expect("state mutex poisoned");
        let Some(members) = state.memberships.get(group) else {
            return Ok(Vec::new());
        };
        let persons: Vec<Person> = members
            .iter()
            .filter_map(|pid| state.persons_by_id.get(pid).cloned())
            .collect();
        Ok(persons)
    }
}
