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
}

impl<S: Subscribe> Server<S> {
    pub fn new(
        subscriber: S,
        store: Arc<dyn EventStore>,
        plan_store: Arc<dyn PlanSource>,
    ) -> Self {
        Self { subscriber, store, plan_store }
    }
}
