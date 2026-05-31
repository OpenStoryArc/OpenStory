//! Live writer for the nats-server account config (Phase 5.4).
//!
//! `AccountConfigWriter` holds the in-memory list of `AccountSpec`s and
//! persists them to a conf file on disk. Reloading nats-server so it picks
//! up the new accounts is a separate concern — exposed as the
//! [`NatsReloader`] trait so tests can use docker-exec and prod can use a
//! POSIX SIGHUP or a NATS control subject.
//!
//! The split-of-concerns mirrors PR #58: pure state mutation + serialization
//! in one type, side-effects (process signaling) in another. Lets the
//! writer be unit-tested without spinning up NATS or a container.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use open_story_bus::accounts::{
    render_accounts_block, AccountSpec, ExportSpec, ImportSpec,
};

/// Mutates and persists the nats-server account config.
///
/// Owns the list of accounts in memory; on `persist()`, writes the
/// `static_prefix` (listen/jetstream/leafnodes) followed by the rendered
/// `accounts { ... }` block to `conf_path`. Atomic via tmpfile + rename.
pub struct AccountConfigWriter {
    conf_path: PathBuf,
    static_prefix: String,
    state: Mutex<Vec<AccountSpec>>,
}

impl AccountConfigWriter {
    pub fn new(
        conf_path: PathBuf,
        static_prefix: impl Into<String>,
        initial: Vec<AccountSpec>,
    ) -> Self {
        Self {
            conf_path,
            static_prefix: static_prefix.into(),
            state: Mutex::new(initial),
        }
    }

    /// Add a one-way share: `from_account` exports `subject` to `to_account`,
    /// `to_account` imports it. Idempotent — adding the same pair twice is a
    /// no-op.
    pub fn add_share(&self, from_account: &str, to_account: &str, subject: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let from = state
            .iter_mut()
            .find(|a| a.name == from_account)
            .ok_or_else(|| anyhow!("source account {from_account} not in config"))?;
        let existing_export = from
            .exports
            .iter_mut()
            .find(|e| e.subject == subject && e.allowed_accounts.contains(&to_account.to_string()));
        if existing_export.is_none() {
            // Try matching subject first — extend allowed_accounts.
            if let Some(e) = from.exports.iter_mut().find(|e| e.subject == subject) {
                e.allowed_accounts.push(to_account.to_string());
            } else {
                from.exports.push(ExportSpec {
                    subject: subject.to_string(),
                    allowed_accounts: vec![to_account.to_string()],
                });
            }
        }

        let to = state
            .iter_mut()
            .find(|a| a.name == to_account)
            .ok_or_else(|| anyhow!("destination account {to_account} not in config"))?;
        let already_imported = to.imports.iter().any(|i| {
            i.from_account == from_account && i.subject == subject && i.to.is_none()
        });
        if !already_imported {
            to.imports.push(ImportSpec {
                from_account: from_account.to_string(),
                subject: subject.to_string(),
                to: None,
            });
        }
        Ok(())
    }

    /// Like [`add_share`] but creates `to_account` as a stub if it doesn't
    /// already exist. The stub has no users — no client can connect *as*
    /// this account locally. Useful for the operator-driven flow where you
    /// declare "I want to share with person X" before person X's credentials
    /// have been provisioned on this node. Actual delivery to X requires
    /// X's credentials to land in the conf (via this writer or by manual
    /// add) so a client can subscribe under that account.
    pub fn add_share_with_stubs(
        &self,
        from_account: &str,
        to_account: &str,
        subject: &str,
    ) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            if !state.iter().any(|a| a.name == to_account) {
                state.push(AccountSpec {
                    name: to_account.to_string(),
                    users: vec![],
                    exports: vec![],
                    imports: vec![],
                });
            }
        }
        self.add_share(from_account, to_account, subject)
    }

    /// Remove a share previously added by [`add_share`]. Idempotent.
    pub fn remove_share(
        &self,
        from_account: &str,
        to_account: &str,
        subject: &str,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(from) = state.iter_mut().find(|a| a.name == from_account) {
            for e in from.exports.iter_mut() {
                if e.subject == subject {
                    e.allowed_accounts.retain(|a| a != to_account);
                }
            }
            from.exports
                .retain(|e| !(e.subject == subject && e.allowed_accounts.is_empty()));
        }
        if let Some(to) = state.iter_mut().find(|a| a.name == to_account) {
            to.imports
                .retain(|i| !(i.from_account == from_account && i.subject == subject));
        }
        Ok(())
    }

    /// Render the current in-memory state as a complete nats-server conf.
    pub fn current_config(&self) -> String {
        let state = self.state.lock().unwrap();
        let block = render_accounts_block(&state);
        format!("{}\n{}", self.static_prefix, block)
    }

    /// Write the current state to disk atomically (tmpfile + rename).
    /// The on-disk file is replaced in one step, so nats-server never sees
    /// a partially-written conf.
    pub fn persist(&self) -> Result<()> {
        let body = self.current_config();
        let parent = self
            .conf_path
            .parent()
            .ok_or_else(|| anyhow!("conf path has no parent: {:?}", self.conf_path))?;
        std::fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(
            ".{}.tmp",
            self.conf_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("nats-conf")
        ));
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.conf_path)?;
        Ok(())
    }
}

/// Trait for "tell nats-server to reread its conf." Implementations: SIGHUP
/// for a locally-owned process, docker-exec for a testcontainer, an HTTP
/// or NATS control subject in a managed deployment.
#[async_trait]
pub trait NatsReloader: Send + Sync {
    async fn reload(&self) -> Result<()>;
}

/// No-op reloader for tests that only check persist() semantics.
pub struct NoopReloader;

#[async_trait]
impl NatsReloader for NoopReloader {
    async fn reload(&self) -> Result<()> {
        Ok(())
    }
}

/// Shell-command reloader. Production default is `pkill -HUP nats-server`,
/// configurable via `Config::nats_reload_command`. Failures (non-zero exit,
/// command not found) bubble up as `Err` so the handler can surface to the
/// operator. Stdout/stderr are captured but discarded — the operator should
/// look at nats-server logs for the actual outcome.
pub struct ShellCommandReloader {
    pub command: String,
}

#[async_trait]
impl NatsReloader for ShellCommandReloader {
    async fn reload(&self) -> Result<()> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "reload command `{}` exited with status {}",
                self.command,
                output.status
            );
        }
        Ok(())
    }
}

/// Static prefix written ahead of the `accounts { ... }` block. Includes
/// the listen socket and JetStream store dir — enough for nats-server to
/// boot. Operators who need leafnodes/clusters/TLS should override the
/// prefix path and manage the file themselves; for local dev this default
/// gets you running with `nats-server -c <managed-conf-path>`.
pub const DEFAULT_NATS_STATIC_PREFIX: &str = "\
# Generated by OpenStory AccountConfigWriter. DO NOT EDIT —
# this file is overwritten on every POST /api/admin/share-with-person.
listen: 0.0.0.0:4222
jetstream {
  store_dir: ./data/nats-jetstream
}";

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_bus::accounts::{AccountSpec, UserSpec};
    use tempfile::TempDir;

    fn person(name: &str, user: &str) -> AccountSpec {
        AccountSpec {
            name: name.into(),
            users: vec![UserSpec {
                user: user.into(),
                password: format!("{user}-secret"),
                permissions: None,
            }],
            exports: vec![],
            imports: vec![],
        }
    }

    fn writer_in(tmp: &TempDir, initial: Vec<AccountSpec>) -> AccountConfigWriter {
        AccountConfigWriter::new(
            tmp.path().join("nats-server.conf"),
            "listen: 0.0.0.0:4222",
            initial,
        )
    }

    #[test]
    fn add_share_produces_paired_export_and_import() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(&tmp, vec![person("PERSON_MAX", "max"), person("PERSON_KATIE", "katie")]);

        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();

        let conf = writer.current_config();
        assert!(conf.contains("{ stream: \"events.session-X.>\", accounts: [PERSON_KATIE] }"));
        assert!(conf.contains(
            "{ stream: { account: PERSON_MAX, subject: \"events.session-X.>\" } }"
        ));
    }

    #[test]
    fn add_share_is_idempotent_for_repeated_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(&tmp, vec![person("PERSON_MAX", "max"), person("PERSON_KATIE", "katie")]);

        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();
        let first = writer.current_config();
        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();
        let second = writer.current_config();
        assert_eq!(first, second);
    }

    #[test]
    fn add_share_with_same_subject_different_target_extends_allowed_accounts() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(
            &tmp,
            vec![
                person("PERSON_MAX", "max"),
                person("PERSON_KATIE", "katie"),
                person("PERSON_BOBBY", "bobby"),
            ],
        );

        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.public.>")
            .unwrap();
        writer
            .add_share("PERSON_MAX", "PERSON_BOBBY", "events.public.>")
            .unwrap();

        let conf = writer.current_config();
        assert!(
            conf.contains("accounts: [PERSON_KATIE, PERSON_BOBBY]"),
            "expected combined allowed_accounts, got:\n{conf}"
        );
    }

    #[test]
    fn remove_share_undoes_add_share() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(&tmp, vec![person("PERSON_MAX", "max"), person("PERSON_KATIE", "katie")]);
        let initial = writer.current_config();

        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();
        writer
            .remove_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();

        assert_eq!(writer.current_config(), initial);
    }

    #[test]
    fn persist_writes_the_current_config_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(&tmp, vec![person("PERSON_MAX", "max"), person("PERSON_KATIE", "katie")]);
        writer
            .add_share("PERSON_MAX", "PERSON_KATIE", "events.session-X.>")
            .unwrap();
        writer.persist().unwrap();

        let on_disk = std::fs::read_to_string(tmp.path().join("nats-server.conf")).unwrap();
        assert_eq!(on_disk, writer.current_config());
    }

    #[test]
    fn persist_emits_the_static_prefix_before_accounts() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = AccountConfigWriter::new(
            tmp.path().join("nats-server.conf"),
            "listen: 0.0.0.0:4222\njetstream { store_dir: /var/lib/nats }",
            vec![person("PERSON_MAX", "max")],
        );
        writer.persist().unwrap();

        let on_disk = std::fs::read_to_string(tmp.path().join("nats-server.conf")).unwrap();
        // Prefix appears before the accounts block.
        let listen_idx = on_disk.find("listen:").expect("listen present");
        let accounts_idx = on_disk.find("accounts {").expect("accounts present");
        assert!(listen_idx < accounts_idx);
        // Both required pieces of the static prefix landed.
        assert!(on_disk.contains("jetstream { store_dir:"));
    }

    #[test]
    fn add_share_rejects_unknown_source_account() {
        let tmp = tempfile::tempdir().unwrap();
        let writer = writer_in(&tmp, vec![person("PERSON_KATIE", "katie")]);
        let err = writer
            .add_share("PERSON_PHANTOM", "PERSON_KATIE", "x")
            .unwrap_err();
        assert!(err.to_string().contains("PERSON_PHANTOM"));
    }
}
