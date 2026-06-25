//! Keycloak-backed `Directory` impl using testcontainers.
//!
//! Spins up `quay.io/keycloak/keycloak:26.0` per test, imports a realm
//! with `sslRequired=none` plus a realm-admin user, mints an admin token
//! against that realm, and exercises the trait through Keycloak's admin
//! REST API.
//!
//! The realm import is necessary because Keycloak 26's `master` realm
//! defaults to `sslRequired=external` — which rejects plain HTTP from a
//! testcontainers-mapped port even though it's loopback. By importing our
//! own realm at startup, we sidestep `master` entirely and operate against
//! a realm whose SSL policy we control.
//!
//! Slow start (~30–60s for the container). Tests are `#[ignore]` by default
//! so CI isn't gated on Docker.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tempfile::TempDir;
use testcontainers::{
    core::{IntoContainerPort, Mount, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use super::*;

const KEYCLOAK_IMAGE: &str = "quay.io/keycloak/keycloak";
const KEYCLOAK_TAG: &str = "26.0";
const TEST_REALM: &str = "openstory-test";
const ADMIN_USER: &str = "admin";
const ADMIN_PASS: &str = "admin";

/// Realm import fixture. SSL not required, one realm-admin user named
/// `admin` with password `admin`. Imported at container startup via
/// `--import-realm`.
const REALM_JSON: &str = r#"{
  "realm": "openstory-test",
  "enabled": true,
  "sslRequired": "none",
  "users": [
    {
      "username": "admin",
      "enabled": true,
      "emailVerified": true,
      "firstName": "Realm",
      "lastName": "Admin",
      "email": "admin@example.test",
      "requiredActions": [],
      "credentials": [{"type": "password", "value": "admin", "temporary": false}],
      "clientRoles": {"realm-management": ["realm-admin"]}
    }
  ]
}"#;

pub struct KeycloakDirectory {
    base_url: String,
    realm: String,
    client: Client,
    admin_token: String,
}

/// Start a Keycloak container, import the test realm, mint an admin token.
///
/// Returns a tuple of (container, tempdir, directory). All three must be
/// kept alive in the caller — dropping the container stops Keycloak;
/// dropping the tempdir removes the realm import file (harmless after
/// Keycloak has read it, but cleanest to keep symmetric with the container
/// lifetime).
pub async fn start_keycloak_directory(
) -> Result<(ContainerAsync<GenericImage>, TempDir, KeycloakDirectory)> {
    // Write the realm import to a tempfile so testcontainers can bind-mount it.
    let realm_dir = tempfile::tempdir().context("failed to create tempdir for realm import")?;
    let realm_path = realm_dir.path().join("realm.json");
    std::fs::write(&realm_path, REALM_JSON).context("failed to write realm.json")?;

    let host_path = realm_path
        .to_str()
        .ok_or_else(|| anyhow!("realm.json path is not valid UTF-8"))?
        .to_string();

    let container = GenericImage::new(KEYCLOAK_IMAGE, KEYCLOAK_TAG)
        .with_exposed_port(8080.tcp())
        .with_wait_for(WaitFor::message_on_stdout("started in"))
        .with_mount(Mount::bind_mount(
            host_path,
            "/opt/keycloak/data/import/realm.json",
        ))
        .with_env_var("KC_BOOTSTRAP_ADMIN_USERNAME", ADMIN_USER)
        .with_env_var("KC_BOOTSTRAP_ADMIN_PASSWORD", ADMIN_PASS)
        .with_cmd(["start-dev", "--import-realm"])
        .start()
        .await
        .context("failed to start Keycloak container")?;

    let port = container
        .get_host_port_ipv4(8080)
        .await
        .context("failed to map Keycloak port")?;
    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let admin_token = get_token(&client, &base_url, TEST_REALM, ADMIN_USER, ADMIN_PASS)
        .await
        .context("failed to obtain admin token from imported realm")?;

    let dir = KeycloakDirectory {
        base_url,
        realm: TEST_REALM.to_string(),
        client,
        admin_token,
    };
    Ok((container, realm_dir, dir))
}

async fn get_token(
    client: &Client,
    base_url: &str,
    realm: &str,
    user: &str,
    pass: &str,
) -> Result<String> {
    let url = format!("{base_url}/realms/{realm}/protocol/openid-connect/token");
    let mut last_err = None;
    for _ in 0..30 {
        let resp = client
            .post(&url)
            .form(&[
                ("client_id", "admin-cli"),
                ("username", user),
                ("password", pass),
                ("grant_type", "password"),
            ])
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let body: Value = r.json().await?;
                let token = body
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("token response missing access_token"))?;
                return Ok(token.to_string());
            }
            Ok(r) => {
                last_err = Some(format!(
                    "{}: {}",
                    r.status(),
                    r.text().await.unwrap_or_default()
                ));
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(anyhow!(
        "could not obtain token from realm {realm} after retries: {:?}",
        last_err
    ))
}

impl KeycloakDirectory {
    fn admin_url(&self, path: &str) -> String {
        format!("{}/admin/realms/{}{}", self.base_url, self.realm, path)
    }

    async fn find_user_id_by_email(&self, email: &str) -> Result<Option<String>> {
        let resp = self
            .client
            .get(self.admin_url("/users"))
            .bearer_auth(&self.admin_token)
            .query(&[("email", email), ("exact", "true")])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("user search failed: {}", resp.status()));
        }
        let users: Vec<Value> = resp.json().await?;
        Ok(users
            .first()
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from))
    }

    async fn find_group_id(&self, group: &GroupId) -> Result<Option<String>> {
        let resp = self
            .client
            .get(self.admin_url("/groups"))
            .bearer_auth(&self.admin_token)
            .query(&[("search", group.0.as_str()), ("exact", "true")])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("group search failed: {}", resp.status()));
        }
        let groups: Vec<Value> = resp.json().await?;
        Ok(groups
            .iter()
            .find(|g| g.get("name").and_then(|v| v.as_str()) == Some(group.0.as_str()))
            .and_then(|g| g.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from))
    }

    async fn ensure_group(&self, group: &GroupId) -> Result<String> {
        if let Some(id) = self.find_group_id(group).await? {
            return Ok(id);
        }
        let resp = self
            .client
            .post(self.admin_url("/groups"))
            .bearer_auth(&self.admin_token)
            .json(&json!({ "name": &group.0 }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "create_group failed: {} - {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        self.find_group_id(group)
            .await?
            .ok_or_else(|| anyhow!("group not found after create"))
    }
}

#[async_trait]
impl Directory for KeycloakDirectory {
    async fn upsert_person(&self, person: NewPerson) -> Result<PersonId> {
        if let Some(existing) = self.find_user_id_by_email(&person.email).await? {
            return Ok(PersonId(existing));
        }
        let resp = self
            .client
            .post(self.admin_url("/users"))
            .bearer_auth(&self.admin_token)
            .json(&json!({
                "username": &person.email,
                "email": &person.email,
                "firstName": &person.display_name,
                "enabled": true,
                "emailVerified": true,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "create_user failed: {} - {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let id = self
            .find_user_id_by_email(&person.email)
            .await?
            .ok_or_else(|| anyhow!("user not found after create"))?;
        Ok(PersonId(id))
    }

    async fn lookup_person(&self, id: &PersonId) -> Result<Option<Person>> {
        let resp = self
            .client
            .get(self.admin_url(&format!("/users/{}", id.0)))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(anyhow!("lookup_person failed: {}", resp.status()));
        }
        let user: Value = resp.json().await?;
        Ok(Some(Person {
            id: id.clone(),
            display_name: user
                .get("firstName")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            email: user
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }))
    }

    async fn add_to_group(&self, person: &PersonId, group: &GroupId) -> Result<()> {
        let kc_group_id = self.ensure_group(group).await?;
        let resp = self
            .client
            .put(self.admin_url(&format!("/users/{}/groups/{}", person.0, kc_group_id)))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "add_to_group failed: {} - {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }

    async fn groups_for_person(&self, person: &PersonId) -> Result<Vec<Group>> {
        let resp = self
            .client
            .get(self.admin_url(&format!("/users/{}/groups", person.0)))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("groups_for_person failed: {}", resp.status()));
        }
        let groups: Vec<Value> = resp.json().await?;
        Ok(groups
            .iter()
            .filter_map(|g| {
                let name = g.get("name").and_then(|v| v.as_str())?.to_string();
                Some(Group {
                    id: GroupId(name.clone()),
                    display_name: name,
                })
            })
            .collect())
    }

    async fn members_of_group(&self, group: &GroupId) -> Result<Vec<Person>> {
        let kc_group_id = match self.find_group_id(group).await? {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };
        let resp = self
            .client
            .get(self.admin_url(&format!("/groups/{}/members", kc_group_id)))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("members_of_group failed: {}", resp.status()));
        }
        let users: Vec<Value> = resp.json().await?;
        Ok(users
            .iter()
            .map(|u| Person {
                id: PersonId(
                    u.get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
                display_name: u
                    .get("firstName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                email: u
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
            .collect())
    }
}
