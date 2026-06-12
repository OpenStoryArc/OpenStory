//! Render NATS server account config blocks for Layer 2 person isolation.
//!
//! Pure functions: typed input → string. No I/O. Side-effects (writing to a
//! conf file, signaling nats-server to reload) live in
//! `open_story_server::account_config::AccountConfigWriter`.
//!
//! ## Why a generator, not a static template
//!
//! Adding a person to the fleet (or sharing one session with another person)
//! adds an account, a user, or an export/import. Hand-editing the conf file
//! at runtime is fragile — operators forget commas, get account names wrong,
//! and there's no way to test what they wrote. A typed generator lets the
//! server compute the right config from `Person` records + `share_policy`
//! rows and emit a deterministic block we can diff against the live file.

use std::fmt::Write as _;

/// A single NATS account: a person's isolated subject space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSpec {
    /// SCREAMING_SNAKE_CASE name as it appears in the conf, e.g. `PERSON_MAX`.
    pub name: String,
    /// Users authorized for this account. Phase 5 uses password auth; Phase 6
    /// will swap in NKEY public keys via a second auth field on `UserSpec`.
    pub users: Vec<UserSpec>,
    /// Subjects this account makes visible to other accounts. Empty by default
    /// (sovereign by default — nothing leaves until the operator says so).
    pub exports: Vec<ExportSpec>,
    /// Subjects this account pulls in from other accounts. The pair
    /// (export on the source account, import on the destination) is required;
    /// either alone doesn't deliver.
    pub imports: Vec<ImportSpec>,
}

/// One user credential. Phase 5 used password-only; Phase 6 adds
/// per-user permission profiles for role-based NATS access (Observer
/// can sub but not pub, Contributor pubs to own subjects, Admin
/// pubs/subs anything).
///
/// `permissions == None` means the user inherits the account's default
/// permissions (no restriction beyond account isolation). `Some(_)`
/// applies the named publish/subscribe allow-lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSpec {
    pub user: String,
    pub password: String,
    pub permissions: Option<PermissionSet>,
}

impl UserSpec {
    /// Convenience constructor for users without per-user permissions
    /// (the default in Phase 5 and earlier).
    pub fn unrestricted(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: password.into(),
            permissions: None,
        }
    }
}

/// NATS permission allow-lists. Empty `publish` denies all publishes;
/// empty `subscribe` denies all subscribes. Use wildcards (`events.>`,
/// `>`) for broad grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionSet {
    pub publish: Vec<String>,
    pub subscribe: Vec<String>,
}

impl PermissionSet {
    /// Observer profile: read-only. Can subscribe to `events.>` but
    /// cannot publish anywhere.
    pub fn observer() -> Self {
        Self {
            publish: vec![],
            subscribe: vec!["events.>".into()],
        }
    }

    /// Contributor profile: can publish AND subscribe under `events.>`
    /// but cannot touch admin or control subjects.
    pub fn contributor() -> Self {
        Self {
            publish: vec!["events.>".into()],
            subscribe: vec!["events.>".into()],
        }
    }

    /// Admin profile: unrestricted within the account.
    pub fn admin() -> Self {
        Self {
            publish: vec![">".into()],
            subscribe: vec![">".into()],
        }
    }
}

/// "Account A is willing to let Account B subscribe to subject S."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSpec {
    /// The subject to export, e.g. `events.session-X.>`.
    pub subject: String,
    /// Accounts allowed to import. If empty, the export is public (any
    /// account can declare a matching import). Sovereign default: empty
    /// — operators name the consent target explicitly.
    pub allowed_accounts: Vec<String>,
}

/// "Account A wants to subscribe to subject S exported by Account B."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpec {
    pub from_account: String,
    pub subject: String,
    /// Optional prefix to mount the imported subject under within this
    /// account's subject space. `None` means import to the same subject.
    pub to: Option<String>,
}

/// Render a complete `accounts { ... }` config block from a list of specs.
///
/// Deterministic: output is byte-stable for a given input order, which makes
/// it easy to diff against the on-disk conf and tell whether a HUP-reload is
/// actually needed.
pub fn render_accounts_block(accounts: &[AccountSpec]) -> String {
    let mut out = String::from("accounts {\n");
    for acc in accounts {
        write_account(&mut out, acc);
    }
    out.push_str("}\n");
    out
}

/// Render a list of NATS subjects as quoted comma-separated tokens —
/// `"events.>", "control.x"`.
fn quoted_subjects(subjects: &[String]) -> String {
    subjects
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render a single permission direction (publish or subscribe) using the
/// explicit allow/deny form NATS expects. The shorthand `publish: [...]`
/// is treated as "no restrictions" when the list is empty — which is the
/// OPPOSITE of what we want for an Observer (who shouldn't publish at
/// all). To make denial explicit, an empty allow-list renders as a deny
/// over `>` instead.
fn render_permission_field(subjects: &[String]) -> String {
    if subjects.is_empty() {
        // Explicit denial — `>` matches every subject.
        "{ deny: [\">\"] }".to_string()
    } else {
        format!("{{ allow: [{}] }}", quoted_subjects(subjects))
    }
}

fn write_account(out: &mut String, acc: &AccountSpec) {
    let _ = writeln!(out, "  {}: {{", acc.name);
    // Explicit accounts default JetStream OFF; each must opt in or stream
    // creation fails ("jetstream not enabled for account").
    out.push_str("    jetstream: enabled\n");

    if !acc.users.is_empty() {
        out.push_str("    users: [\n");
        for u in &acc.users {
            match &u.permissions {
                None => {
                    let _ = writeln!(
                        out,
                        "      {{ user: \"{}\", password: \"{}\" }}",
                        u.user, u.password
                    );
                }
                Some(p) => {
                    let _ = writeln!(
                        out,
                        "      {{ user: \"{}\", password: \"{}\", permissions: {{ publish: {}, subscribe: {} }} }}",
                        u.user,
                        u.password,
                        render_permission_field(&p.publish),
                        render_permission_field(&p.subscribe),
                    );
                }
            }
        }
        out.push_str("    ]\n");
    }

    if !acc.exports.is_empty() {
        out.push_str("    exports: [\n");
        for e in &acc.exports {
            if e.allowed_accounts.is_empty() {
                let _ = writeln!(out, "      {{ stream: \"{}\" }}", e.subject);
            } else {
                let _ = writeln!(
                    out,
                    "      {{ stream: \"{}\", accounts: [{}] }}",
                    e.subject,
                    e.allowed_accounts.join(", ")
                );
            }
        }
        out.push_str("    ]\n");
    }

    if !acc.imports.is_empty() {
        out.push_str("    imports: [\n");
        for i in &acc.imports {
            match &i.to {
                None => {
                    let _ = writeln!(
                        out,
                        "      {{ stream: {{ account: {}, subject: \"{}\" }} }}",
                        i.from_account, i.subject
                    );
                }
                Some(to) => {
                    let _ = writeln!(
                        out,
                        "      {{ stream: {{ account: {}, subject: \"{}\" }}, to: \"{}\" }}",
                        i.from_account, i.subject, to
                    );
                }
            }
        }
        out.push_str("    ]\n");
    }

    out.push_str("  }\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person_max() -> AccountSpec {
        AccountSpec {
            name: "PERSON_MAX".into(),
            users: vec![UserSpec {
                user: "max".into(),
                password: "max-secret".into(),
                permissions: None,
            }],
            exports: vec![],
            imports: vec![],
        }
    }

    fn person_katie() -> AccountSpec {
        AccountSpec {
            name: "PERSON_KATIE".into(),
            users: vec![UserSpec {
                user: "katie".into(),
                password: "katie-secret".into(),
                permissions: None,
            }],
            exports: vec![],
            imports: vec![],
        }
    }

    #[test]
    fn every_account_enables_jetstream() {
        // When accounts are explicitly defined, nats-server scopes JetStream
        // per account and defaults it OFF — without `jetstream: enabled` the
        // server's stream creation fails with "jetstream not enabled for
        // account" even though the global jetstream block exists.
        let out = render_accounts_block(&[person_max(), person_katie()]);
        assert_eq!(
            out.matches("jetstream: enabled").count(),
            2,
            "each account must opt into JetStream:\n{out}"
        );
    }

    #[test]
    fn empty_input_renders_empty_block() {
        let out = render_accounts_block(&[]);
        assert_eq!(out, "accounts {\n}\n");
    }

    #[test]
    fn two_isolated_accounts_have_no_exports_or_imports() {
        let out = render_accounts_block(&[person_max(), person_katie()]);
        assert!(out.contains("PERSON_MAX: {"));
        assert!(out.contains("PERSON_KATIE: {"));
        assert!(out.contains("user: \"max\", password: \"max-secret\""));
        assert!(out.contains("user: \"katie\", password: \"katie-secret\""));
        assert!(!out.contains("exports"));
        assert!(!out.contains("imports"));
    }

    #[test]
    fn export_with_allowed_account_names_the_consent_target() {
        let mut max = person_max();
        max.exports.push(ExportSpec {
            subject: "events.session-X.>".into(),
            allowed_accounts: vec!["PERSON_KATIE".into()],
        });
        let out = render_accounts_block(&[max]);
        assert!(out.contains(
            "{ stream: \"events.session-X.>\", accounts: [PERSON_KATIE] }"
        ));
    }

    #[test]
    fn export_without_allowed_accounts_is_public() {
        let mut max = person_max();
        max.exports.push(ExportSpec {
            subject: "events.public.>".into(),
            allowed_accounts: vec![],
        });
        let out = render_accounts_block(&[max]);
        assert!(out.contains("{ stream: \"events.public.>\" }"));
        // Specifically, no `accounts: [...]` clause.
        assert!(!out.contains("accounts: ["));
    }

    #[test]
    fn import_pairs_an_export_on_the_other_side() {
        // Max exports session-X to Katie; Katie imports it.
        let mut max = person_max();
        max.exports.push(ExportSpec {
            subject: "events.session-X.>".into(),
            allowed_accounts: vec!["PERSON_KATIE".into()],
        });
        let mut katie = person_katie();
        katie.imports.push(ImportSpec {
            from_account: "PERSON_MAX".into(),
            subject: "events.session-X.>".into(),
            to: None,
        });
        let out = render_accounts_block(&[max, katie]);
        assert!(out.contains(
            "{ stream: { account: PERSON_MAX, subject: \"events.session-X.>\" } }"
        ));
    }

    #[test]
    fn import_with_to_prefix_mounts_under_a_local_namespace() {
        let mut katie = person_katie();
        katie.imports.push(ImportSpec {
            from_account: "PERSON_MAX".into(),
            subject: "events.session-X.>".into(),
            to: Some("shared.max".into()),
        });
        let out = render_accounts_block(&[katie]);
        assert!(out.contains(
            "{ stream: { account: PERSON_MAX, subject: \"events.session-X.>\" }, to: \"shared.max\" }"
        ));
    }

    #[test]
    fn output_is_deterministic_for_a_given_input_order() {
        let one = render_accounts_block(&[person_max(), person_katie()]);
        let two = render_accounts_block(&[person_max(), person_katie()]);
        assert_eq!(one, two);
    }

    // ── Phase 6.7 — Per-user permission profiles ────────────────────────

    #[test]
    fn user_without_permissions_renders_as_plain_password_entry() {
        let acc = person_max();
        let out = render_accounts_block(&[acc]);
        // No `permissions:` clause when None.
        assert!(!out.contains("permissions:"));
        assert!(out.contains("{ user: \"max\", password: \"max-secret\" }"));
    }

    #[test]
    fn observer_user_can_subscribe_but_not_publish() {
        let mut acc = person_max();
        acc.users[0].permissions = Some(PermissionSet::observer());
        let out = render_accounts_block(&[acc]);
        // Empty publish renders as explicit deny — the shorthand
        // `publish: []` is treated as "no restriction" by NATS, which
        // is the opposite of what an Observer wants.
        assert!(
            out.contains("publish: { deny: [\">\"] }"),
            "observer publish should be explicit deny; got:\n{out}"
        );
        assert!(out.contains("subscribe: { allow: [\"events.>\"] }"));
    }

    #[test]
    fn contributor_user_scoped_to_events() {
        let mut acc = person_max();
        acc.users[0].permissions = Some(PermissionSet::contributor());
        let out = render_accounts_block(&[acc]);
        assert!(out.contains("publish: { allow: [\"events.>\"] }"));
        assert!(out.contains("subscribe: { allow: [\"events.>\"] }"));
    }

    #[test]
    fn admin_user_unrestricted_within_account() {
        let mut acc = person_max();
        acc.users[0].permissions = Some(PermissionSet::admin());
        let out = render_accounts_block(&[acc]);
        assert!(out.contains("publish: { allow: [\">\"] }"));
        assert!(out.contains("subscribe: { allow: [\">\"] }"));
    }

    #[test]
    fn three_users_with_three_distinct_roles_render_inline() {
        let acc = AccountSpec {
            name: "PERSON_MAX".into(),
            users: vec![
                UserSpec {
                    user: "max-admin".into(),
                    password: "p1".into(),
                    permissions: Some(PermissionSet::admin()),
                },
                UserSpec {
                    user: "max-bot".into(),
                    password: "p2".into(),
                    permissions: Some(PermissionSet::contributor()),
                },
                UserSpec {
                    user: "max-readonly".into(),
                    password: "p3".into(),
                    permissions: Some(PermissionSet::observer()),
                },
            ],
            exports: vec![],
            imports: vec![],
        };
        let out = render_accounts_block(&[acc]);
        assert!(out.contains("user: \"max-admin\""));
        assert!(out.contains("user: \"max-bot\""));
        assert!(out.contains("user: \"max-readonly\""));
        // Observer (last user) has explicit deny on publish.
        assert!(out.contains(
            "user: \"max-readonly\", password: \"p3\", permissions: { publish: { deny: [\">\"] }, subscribe: { allow: [\"events.>\"] }"
        ));
    }

    #[test]
    fn quoted_subjects_handles_multiple_entries() {
        assert_eq!(quoted_subjects(&[]), "");
        assert_eq!(
            quoted_subjects(&["a".into(), "b.>".into()]),
            "\"a\", \"b.>\""
        );
    }
}
