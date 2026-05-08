//! Directory pluggability spike — entry point.
//!
//! Validates that the `Directory` trait is implementable against both an
//! embedded backend and an external IdP (Keycloak via testcontainers), with
//! both passing the same conformance scenarios.
//!
//! Marked `#[ignore]` so CI isn't gated on Docker. Run with:
//!
//! ```text
//! cargo test -p open-story-server --test directory_pluggability -- --ignored
//! ```
//!
//! See `docs/research/personhood-and-principals.md` for context.
//!
//! Status:
//! - [x] step 1 — trait + types + BDD scenario
//! - [x] step 2 — embedded impl + green against scenario
//! - [x] step 3 — keycloak impl + green against scenario (requires Docker)

mod directory;

#[tokio::test]
async fn embedded_directory_conformance() {
    let dir = directory::embedded::EmbeddedDirectory::new();
    directory::conformance::run_full_suite(&dir)
        .await
        .expect("embedded directory should pass full conformance suite");
}

#[tokio::test]
#[ignore = "requires Docker — run explicitly with `-- --ignored`"]
async fn keycloak_directory_conformance() {
    let (_keycloak, _realm_dir, dir) = directory::keycloak::start_keycloak_directory()
        .await
        .expect("keycloak should start in testcontainer");
    directory::conformance::run_full_suite(&dir)
        .await
        .expect("keycloak directory should pass full conformance suite");
}
