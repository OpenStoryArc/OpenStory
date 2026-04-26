Feature: CloudEvent::new Typed EventData Migration
  Complete the half-finished refactor from serde_json::Value to typed EventData
  so that CI is green and new features can land safely.

  Background:
    Given the CloudEvent type requires EventData (not raw serde_json::Value)
    And the AgentPayload enum has ClaudeCode and PiMono variants
    And existing helpers make_event_data and to_cloud_event demonstrate the fix pattern

  Scenario: Server ingest test fixtures use typed EventData
    Given the 7 CloudEvent::new call sites in rs/server/src/ingest.rs
    When each call site is updated to use EventData::new(raw, seq, session_id)
    Then cargo test -p open-story-server compiles successfully
    And all ingest tests pass

  Scenario: Persist consumer test fixtures use current constructor signatures
    Given the 6 constructor call sites in rs/server/src/consumers/persist.rs
    When PersistConsumer::new is called with (store, session_store) arguments
    And SessionStore::new is called with &Path (not PathBuf)
    And Result returns are unwrapped with .expect()
    Then cargo test -p open-story-server compiles successfully
    And all persist consumer tests pass

  Scenario: Integration test fixtures use typed EventData
    Given the 6 integration test files in rs/tests/
    When each CloudEvent::new call site is updated to use EventData::new(...)
    And test helpers in rs/tests/helpers/mod.rs use typed constructors
    Then cargo test --workspace --exclude open-story-cli compiles successfully
    And all integration tests pass

  Scenario: No regressions in already-fixed crates
    Given the views, store, and bus crates already compile with typed EventData
    When the server and integration test fixes are applied
    Then cargo test -p open-story-views still passes
    And cargo test -p open-story-store still passes
    And cargo test -p open-story-bus still passes

  Scenario: Full CI suite is green
    When just test is executed
    Then cargo test --workspace --exclude open-story-cli passes
    And npm test passes
    And cargo clippy passes with no warnings
