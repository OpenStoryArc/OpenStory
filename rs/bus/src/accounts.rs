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

/// One user credential. Phase 5 = password; Phase 6 will add NKEY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSpec {
    pub user: String,
    pub password: String,
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

fn write_account(out: &mut String, acc: &AccountSpec) {
    let _ = writeln!(out, "  {}: {{", acc.name);

    if !acc.users.is_empty() {
        out.push_str("    users: [\n");
        for u in &acc.users {
            let _ = writeln!(
                out,
                "      {{ user: \"{}\", password: \"{}\" }}",
                u.user, u.password
            );
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
            }],
            exports: vec![],
            imports: vec![],
        }
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
}
