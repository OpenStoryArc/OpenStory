//! BDD conformance scenarios for the Directory trait.
//!
//! Each scenario is a free function that takes any `Directory` impl and
//! exercises one well-defined behavior. `run_full_suite` runs them all
//! sequentially against one backend instance — scenarios use distinct
//! email/group names so state doesn't collide between scenarios.
//!
//! Scenarios use abstract test data (Alice, Bob, `example.test` domain) —
//! no project context. Conformance describes *the shape of the trait*,
//! not any particular use case.

use super::*;
use anyhow::Context as _;
use std::collections::HashSet;

/// Run every conformance scenario against a single backend instance.
///
/// Each scenario annotates its failures with `.context(...)` so the test
/// report says *which* scenario failed, not just which assertion.
pub async fn run_full_suite<D: Directory>(directory: &D) -> Result<()> {
    two_persons_in_one_group(directory)
        .await
        .context("scenario: two persons in one group")?;
    idempotent_operations(directory)
        .await
        .context("scenario: idempotent operations")?;
    person_in_multiple_groups(directory)
        .await
        .context("scenario: person in multiple groups")?;
    lookup_misses(directory)
        .await
        .context("scenario: lookup misses")?;
    Ok(())
}

/// Scenario: two persons in one group.
///
/// Exercises all five trait methods in one happy-path flow.
pub async fn two_persons_in_one_group<D: Directory>(directory: &D) -> Result<()> {
    let alice_id = directory
        .upsert_person(NewPerson {
            display_name: "Alice".into(),
            email: "alice@example.test".into(),
        })
        .await?;
    let bob_id = directory
        .upsert_person(NewPerson {
            display_name: "Bob".into(),
            email: "bob@example.test".into(),
        })
        .await?;

    let members = GroupId("members".into());
    directory.add_to_group(&alice_id, &members).await?;
    directory.add_to_group(&bob_id, &members).await?;

    let alice = directory
        .lookup_person(&alice_id)
        .await?
        .expect("alice should be retrievable after upsert");
    assert_eq!(alice.display_name, "Alice");
    assert_eq!(alice.email, "alice@example.test");

    let bob = directory
        .lookup_person(&bob_id)
        .await?
        .expect("bob should be retrievable after upsert");
    assert_eq!(bob.display_name, "Bob");

    let alice_groups = directory.groups_for_person(&alice_id).await?;
    assert_eq!(alice_groups.len(), 1, "alice should be in exactly one group");
    assert_eq!(alice_groups[0].id, members);

    let bob_groups = directory.groups_for_person(&bob_id).await?;
    assert_eq!(bob_groups.len(), 1, "bob should be in exactly one group");
    assert_eq!(bob_groups[0].id, members);

    let group_members = directory.members_of_group(&members).await?;
    assert_eq!(group_members.len(), 2, "group should contain both persons");
    let member_ids: HashSet<&PersonId> = group_members.iter().map(|p| &p.id).collect();
    assert!(member_ids.contains(&alice_id), "group should contain alice");
    assert!(member_ids.contains(&bob_id), "group should contain bob");

    Ok(())
}

/// Scenario: idempotent operations.
///
/// `upsert_person` must be idempotent on email — re-upserting the same email
/// returns the existing `PersonId`. `add_to_group` must be idempotent —
/// adding the same person to the same group twice produces a single
/// membership, not two.
pub async fn idempotent_operations<D: Directory>(directory: &D) -> Result<()> {
    // upsert_person idempotency
    let id_first = directory
        .upsert_person(NewPerson {
            display_name: "Idem".into(),
            email: "idem@example.test".into(),
        })
        .await?;
    let id_second = directory
        .upsert_person(NewPerson {
            // Different display name — same email.
            // (Keep name simple: Keycloak's User Profile feature rejects
            // certain special characters in firstName.)
            display_name: "Idem Updated".into(),
            email: "idem@example.test".into(),
        })
        .await?;
    assert_eq!(
        id_first, id_second,
        "upsert_person must be idempotent on email"
    );

    // add_to_group idempotency
    let group = GroupId("idem-group".into());
    directory.add_to_group(&id_first, &group).await?;
    directory.add_to_group(&id_first, &group).await?;
    let members = directory.members_of_group(&group).await?;
    assert_eq!(
        members.len(),
        1,
        "add_to_group must be idempotent — duplicate add must not produce duplicate membership"
    );

    Ok(())
}

/// Scenario: a person can belong to multiple groups.
///
/// Exercises the cross-group correctness — `groups_for_person` must return
/// every group the person belongs to; `members_of_group` for each group
/// must include the person.
pub async fn person_in_multiple_groups<D: Directory>(directory: &D) -> Result<()> {
    let alice_id = directory
        .upsert_person(NewPerson {
            display_name: "Alice Multi".into(),
            email: "alice-multi@example.test".into(),
        })
        .await?;

    let g1 = GroupId("group-alpha".into());
    let g2 = GroupId("group-beta".into());
    directory.add_to_group(&alice_id, &g1).await?;
    directory.add_to_group(&alice_id, &g2).await?;

    let groups = directory.groups_for_person(&alice_id).await?;
    assert_eq!(groups.len(), 2, "alice should be in both groups");
    let ids: HashSet<&GroupId> = groups.iter().map(|g| &g.id).collect();
    assert!(ids.contains(&g1), "groups should contain group-alpha");
    assert!(ids.contains(&g2), "groups should contain group-beta");

    for g in [&g1, &g2] {
        let members = directory.members_of_group(g).await?;
        let member_ids: HashSet<&PersonId> = members.iter().map(|p| &p.id).collect();
        assert!(
            member_ids.contains(&alice_id),
            "group {:?} should contain alice",
            g
        );
    }

    Ok(())
}

/// Scenario: lookup misses return empty, not error.
///
/// A miss is not a failure — it's a valid answer. The trait contract says
/// `lookup_person` for an unknown id returns `Ok(None)`, and
/// `members_of_group` for an unknown group returns `Ok(vec![])`. Backends
/// that surface 404s as errors must translate them to these shapes.
pub async fn lookup_misses<D: Directory>(directory: &D) -> Result<()> {
    // lookup_person on a non-existent id
    let missing = directory
        .lookup_person(&PersonId("00000000-0000-0000-0000-000000000000".into()))
        .await?;
    assert!(
        missing.is_none(),
        "lookup_person on a missing id must return Ok(None), not Err"
    );

    // members_of_group on a never-created group
    let no_members = directory
        .members_of_group(&GroupId("never-created-group".into()))
        .await?;
    assert!(
        no_members.is_empty(),
        "members_of_group on a missing group must return Ok([]), not Err"
    );

    Ok(())
}
