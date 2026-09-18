//! `Server` — the context every MCP tool dispatcher sees.
//!
//! The streaming tools (`subscribe_session`, `subscribe_tokens`)
//! need a `Subscribe` impl. The query tools (`session_synopsis`,
//! `tool_journey`, `search`, …) need an `EventStore`. Carrying both
//! through one struct keeps `stdio::run` monomorphic and lets the
//! dispatch layer hand the right capability to the right tool
//! without each call site re-wiring the world.
//!
//! Production wires `NatsBus` + `HttpEventStore` + `HttpPlanSource`
//! (the MCP reads through the REST server). Tests pass
//! `LoopbackSubscriber` + a temp-dir `SqliteStore` + `PlanStore` from
//! `tests/common/store_fixture.rs` — `Arc<PlanStore>` coerces to
//! `Arc<dyn PlanSource>` at the call site, so test wiring is unchanged.

use crate::plan_source::PlanSource;
use crate::subscription::Subscribe;
use open_story_store::event_store::EventStore;
use std::sync::Arc;

#[derive(Clone)]
pub struct Server<S: Subscribe> {
    pub subscriber: S,
    pub store: Arc<dyn EventStore>,
    pub plan_store: Arc<dyn PlanSource>,
    /// REST origin (e.g. `http://localhost:3002`) for the agent-in-UI WRITE
    /// seam — control tools POST here to drive the dashboard. Empty means "not
    /// configured" (query/streaming tools don't need it; control tools error
    /// clearly). Set via `with_api_base` so the 3-arg `new` stays churn-free.
    pub api_base: String,
    /// Channel mode (memory hands, E-12): when set, the server declares the
    /// research-preview `claude/channel` capability, auto-subscribes to
    /// story arcs on connect, and pushes each closed node as
    /// `notifications/claude/channel`, so the stream lands in a Claude Code
    /// session with no tool call. `None` leaves behaviour unchanged.
    pub channel: Option<ChannelConfig>,
}

/// What the channel follows: one session, or every session (`None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelConfig {
    pub session_id: Option<String>,
}

impl<S: Subscribe> Server<S> {
    pub fn new(subscriber: S, store: Arc<dyn EventStore>, plan_store: Arc<dyn PlanSource>) -> Self {
        Self {
            subscriber,
            store,
            plan_store,
            api_base: String::new(),
            channel: None,
        }
    }

    /// Set the REST origin control tools POST to. Chainable at the bin call site.
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    /// Turn on channel mode. Chainable at the bin call site.
    pub fn with_channel(mut self, channel: ChannelConfig) -> Self {
        self.channel = Some(channel);
        self
    }
}
