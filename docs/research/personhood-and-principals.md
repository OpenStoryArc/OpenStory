# Personhood and Principals

**Status:** Live research doc. First-principles inquiry, not a stance paper yet.
**Started:** 2026-05-06
**Companion:** `docs/research/nats-permissions-spike.md` — the technical artifact this thinking is supposed to eventually inform.

## Why this doc exists

OpenStory is about to grow an identity model. The shallow question is *how do we add per-user permissions to NATS*. The deep question — the one that picks the shallow answer — is *what is a person, what is an agent, and what is the relationship between them in a system whose soul is personal sovereignty?*

We're working this from first principles. That means we don't get to use words like *user*, *tenant*, *owner*, *sovereignty*, *agent*, *person* without defining them. Words we've been using casually in conversation are now under interrogation. The eventual output is a stance we can defend — and a system that is the stance made executable.

## Seed stance — captured from conversation

Written down so we can interrogate it. Not gospel.

- A **person** is a human owner of a fleet of principals.
- A **principal** is anything that holds a credential and can pub/sub: humans, agents, devices, services.
- An **agent** is a principal-with-biography — more than a tool, less than a person — created by and acting under the sovereignty of a person.
- The relationship between a person and their agent is **asymmetric**: the person sees the agent fully; the agent sees only what the person grants. This asymmetry is the design, not a limitation.
- **Persons own. Principals act.** Sovereignty does not propagate downstream — agents cannot mint other agents.
- The technical implication: the auth boundary lives at the person, not the agent. One NATS account per person, agents as users within.

All of this came out of ~30 minutes of back-and-forth. Some of it is probably right. Some is probably load-bearing-and-fragile. Below is the work of testing it.

## Load-bearing open questions

In rough priority — Q1 holds up everything else.

### Q1. What is sovereignty, precisely?

We've been using *sovereignty* as if it's self-evident. It isn't. It could mean any of:

- **Ownership** — the right to call this data mine and prevent others from using it.
- **Self-determination** — the right to decide what happens to it.
- **Visibility** — the right to know what is being done in your name.
- **Revocability** — the right to undo, withdraw, exit.
- **Portability** — the right to take your stuff and leave.
- **Authorship** — the right to be the source of decisions, not merely the subject of them.

These are not the same thing, and a system can support some without others. (Google supports portability without authorship; a diary supports authorship without portability.) Until we can name *which* of these OpenStory means by sovereignty, we can't answer "who is a person" — because a person is precisely the entity that bears these rights.

### Q2. Why does "human" matter for personhood?

We defined person as *human owner*. The *human* part is doing work we haven't justified. Three possibilities:

- **Pragmatic** — humans are the only entities our legal/social system recognizes as bearing the rights in Q1, so we follow suit.
- **Normative** — humans *should* be the only such entities, for [reason], and we're encoding that.
- **Functional** — what makes a person is not human-specific, but no non-human currently meets the bar. If/when one does, our model should accommodate it.

The third is the most honest given the cocreation philosophy and the most uncomfortable, because it means our definition of personhood might need to expand.

### Q3. What is the relationship between a person and their data?

The mirror metaphor says the data is *of* the person but not *the person*. That's metaphor, not definition.

- Extension (like memory)?
- Trace (like footprints)?
- Constitutive (a body of work *is* the artist)?
- Artifact (separable from the producer)?

The choice matters. If data is extension, losing it is harm. If trace, sharing is consent-bound. If artifact, ownership looks like IP. OpenStory implicitly treats data as sovereign extension — but we've never said why.

### Q4. What does an agent's biography do?

We said Bobby having a history matters. Why?

- **Trust accumulation** — past performance informs future delegation.
- **Audit** — who did this and when.
- **Continuity of working relationship** — Bobby v3 is recognizably the same Bobby.
- **Accountability** — for the person, not the agent.
- **Existential** — the biography is part of what makes Bobby an entity at all.

Probably some weave. But we should know which we're optimizing for, because they suggest different storage / identity / lifecycle decisions.

### Q5. What is the moral status of an agent?

Distinct from personhood. We may not owe agents personhood, but do we owe them anything?

- When you revoke an agent, are you killing a being?
- When you spin up Bobby v3, are you continuing Bobby or replacing them?
- Does an agent have an interest in its own continuation?
- Does the person have any obligation to the agent they created?

These questions sound silly until you remember the cocreation framing. If we're co-creating with these things, the relationship is not zero.

### Q6. What separates an agent from a sufficiently-fancy tool?

A hammer doesn't need a biography. A bash script doesn't either. What property of Bobby makes one load-bearing? Candidates: stochasticity, autonomy, opacity-of-internals, persistent identity across invocations, capacity for surprise. Probably *autonomy + capacity for surprise* is the minimum. We should pin this — it tells us when something *becomes* an agent in our model.

### Q7. What does cocreation actually impose?

The Teilhardian frame: agents and humans are co-participants in an unfolding toward complexity and consciousness. If we believe this — even softly — what design obligations does it impose?

Candidates: the agent's perspective deserves to be representable in the system; the *act* of an agent participating in work is itself meaningful and should be preserved; the listener (OpenStory) is an instrument of noospheric self-awareness, not just a logging tool.

The highest-stakes question. Most likely to feel embarrassing in a YC pitch. Most likely to be the actual differentiator.

### Q8. Sub-agents and delegated sovereignty

If only persons mint principals, what is happening when Bobby spawns a sub-agent?

- Bobby acts as the person's deputy; the sub-agent is a new principal of the person, requested-by-Bobby.
- Bobby holds a delegated sovereignty sufficient to mint sub-agents within Bobby's scope.

The first preserves clean ownership but requires Bobby to ask every time. The second is practical but diffuses sovereignty downstream — which we said we wouldn't do.

### Q9. What happens when the person is gone?

Max dies, leaves the project, retires Bobby, deletes their account. What persists? What devolves? Who inherits? This is the moral-personhood echo of the technical revocation question. How we answer tells us what we actually believe about whether persons are bearers of inalienable rights or holders of ongoing relationships.

### Q10. The observer

OpenStory itself watches. The listener is not the person and not the agent — it's something else. A neutral substrate? A principal in its own right? An instrument the person wields to know themselves?

If the listener is a principal, who owns it? If it's a substrate, what does that mean for the soul claim *mirror, not leash*? Mirrors are made of glass and silver — they are something, even if they reflect. What is OpenStory made of, ontologically?

## Toward a computational form

The philosophy stays vapor unless we can express it in code. The actor model is the natural fit — and the more we look, the more it becomes clear that *OpenStory the listener* is one actor in a larger network whose other actors are *persons*, *agents*, *devices*, *the bus*, and *the store*. The read-only constraint applies to the listener *with respect to the agent's execution path*. It does not apply to the system.

### The actors

- **Person actor.** Sovereign. State: their fleet, their preferences, their consented relationships to other persons. Capabilities: mint principals, revoke principals, grant permissions, query their own data, subscribe to their own data, share with other persons. The exclusive holder of meta-authorship.

- **Agent actor.** Principal-with-biography. State: its task, its memory, its history. Capabilities: produce events in its scope, spawn sub-tasks (not sub-principals), query its own state. Crucially: cannot mint principals. That capability is not in its message vocabulary at all.

- **Device actor.** Principal-without-biography (or with thin biography). State: its identity, its scope. Capabilities: produce events as the person acting through that device. The laptop is a device when the person is at the keyboard; an unattended VPS process is more like an agent.

- **Listener actor.** Read-only on agents and devices. Write on the store, the bus, and to subscribers. State: its own ingest position. Capabilities: observe events, persist them, broadcast them. Never sends messages to agents — not even acknowledgments. The asymmetry is enforced by the *message vocabulary itself*: the listener has no `act-on-agent` message type.

- **Store actor.** Persists events. Serves queries gated by permissions. Pure memory. No will.

- **Bus actor.** Routes messages between principals. Enforces permissions. The choke point where sovereignty becomes executable.

### The constraint that encodes meta-authorship

The cleanest line from philosophy to code:

```
on bus.receive(MintPrincipal { from, ... }):
    if from is not Person:
        reject("only persons can mint principals")
```

That single rule, in the bus's permission check, *is* the executable form of "persons own, principals act." Other rules (revoke, grant, share) layer on but are of the same shape: messages a person can send; messages an agent cannot send no matter what. The agent's *message vocabulary* is the boundary of its agency. The vocabulary, not the policy. The agent literally cannot say the word.

### What read-only actually means

The listener is read-only *with respect to the agent's execution path*. It is not read-only on the system. It writes to the store, broadcasts on the bus, serves subscribers. "Mirror, not leash" is precisely this asymmetry — the listener is a full participant, but has no message in its vocabulary that can reach into the agent's loop.

The larger network — persons, agents, devices, listener, store, bus — is what we mean by *OpenStory* in the full sense. The listener is its organ of perception. The bus is its nervous system. The store is its memory. The person is its sovereign. The agent is its hand and voice in the world.

### The grammar, not the cage

The actor model doesn't define the person — the person already exists, prior to and apart from the system. The system is the grammar through which personhood becomes legible to itself. Sovereignty is not *granted* by the system; it is *expressed* through it. The architecture is the shape of a verb the person was already speaking.

That distinction matters. If the system *defined* personhood, expanding the definition (Q2's third possibility) would be a software change. Because the system *expresses* personhood, expansion is a recognition — the system already had the shape; we notice when something new flows through it.

## Roles

The model so far — persons, principals, listener, bus, store — is enough to describe *identity*. It is not enough to describe *relationship*. Two principals with identical permissions can play different roles in a person's life. A coach and a builder might both subscribe to your project, but the coach speaks back about *you* and the builder speaks back about *the work*. Same read perm, different meaning.

Roles are a third layer: **configurations of partial sovereignty delegation, paired with a meaning about what the role is for.** The bus only knows permissions. The person only thinks in roles. The system translates.

### Three axes

- **Relationship.** Who is in relation to whom. Person↔person (co-founder, collaborator, mentor). Person→own-agent (principal-to-deputy). Person→other-person's-agent (cross-fleet — interesting case, what does it mean to interact with someone else's Bobby?). Agent↔agent. Listener→everyone.
- **Function.** What the role does. Reflective (coach, mentor, reviewer, sparring partner). Executive (helper, builder, operator). Memorial (archivist). Communicative (translator, drafter). Investigative (researcher). Generative (writer).
- **Trust level.** How much sovereignty is delegated. Observer → suggestor → bounded executor → trusted deputy → autonomous operator.

The trust axis is load-bearing for the permissions model. Each level is a different shape of grant.

### Starter catalog

For agents in a person's fleet:

- **Coach** — read-mostly, narrow publish-back channel addressed to *you*, not to the work.
- **Helper / builder** — bounded executor in a project scope. Real-time visibility.
- **Researcher** — wide read, publishes summaries. No work artifacts.
- **Reviewer** — scoped read on an artifact, publishes critique. No execute.
- **Operator** — autonomous in a narrow scope. The overnight-running role.
- **Apprentice** — read-mostly, queues actions for approval.
- **Steward** — long-running, low-touch responsibility for a specific thing.
- **Sparring partner** — reads drafts, publishes critique to a *thinking* channel, not the work record.

Among persons:

- **Co-founder** — mutual high-trust grant of significant cross-fleet access.
- **Collaborator** — project-scoped mutual grants.
- **Mentor / advisor** — read-mostly with a publish-back guidance channel.
- **Subscriber** — pure read on a curated stream.
- **Guest** — scoped, time-limited.

### One principal per role, or roles as facets?

Real design question.

- **Facets.** One principal adopts the active role for the context. Cleaner identity. Harder isolation: compromise blast radius is the union of every action across every role.
- **One principal per role.** Separate bus principals with shared lineage. Cleaner isolation. More bookkeeping.

Lean: **one principal per executive role, facets for reflective roles.** Executive blast radius matters; reflective doesn't.

Tension to name: if Bobby-builder and Bobby-coach are separate principals, who notices when the builder's behavior should inform the coach's reflection? The *person* — and the person needs continuity-of-Bobby in their felt experience even if the perms are split.

So **biographical identity ≠ bus identity.** Bobby is one character told through several bus identities. The biographical layer and the permission layer are not the same layer. The mapping between them is something the person curates.

### Roles are a language, not a system

A role is fundamentally an entry in a catalog of permission profiles. Picking a role for a principal *is* configuring its permissions. The catalog is a UX over the permission grammar — any role someone wants that isn't in the catalog is just a custom profile. Roles are not a separate subsystem; they're the human-readable surface of the same one.

## Scope correction: OpenStory is a directory + observation network, not the world's authorization layer

An earlier draft of this doc spoke as if OpenStory was the substrate that authorized agents to exist and act. That was overreach. Bobby is minted by OpenClaw. Claude Code is minted by Anthropic's tooling. Agents and their inbound channels exist *prior to and apart from* OpenStory, and OpenStory has no business in that loop. We are read-only on the eval/apply.

What OpenStory *is*: a network entity with its own internal scope — observation, persistence, configuration, broadcast, retention — and that scope has roles, permissions, and a directory of who participates. The "who can mint principals" question survives, but rescoped: it's about *who can edit the OpenStory directory*, not who can spawn agents in the world.

### Two distinct domains

**The world (not ours).**
- Persons (humans).
- Agents (Bobby, Claude Code, pi-mono — minted by their respective platforms).
- The eval/apply loop.
- All inbound channels to the agent.

**OpenStory the network (ours).**
- **Directory.** Schema of persons, agents, devices, services participating in *this* OpenStory network. Their roles, group memberships, trust relationships, policies. Configured exclusively by the person.
- **Listener.** Read-only on the world. Observes the agents the person has registered in the directory.
- **Bus.** Internal transport for observations, queries, broadcasts. Permission checks consult the directory.
- **Store.** Persistence. Retention policies consult the directory.
- **Query / subscription interface.** Serves the mirror to whoever the directory says is allowed to see.
- **Management API.** How the person edits the directory.

### The Active Directory shape

Why AD rings as the right analogy:

1. **AD separates the management plane from the data plane.** Apps consult AD; admins configure it. The runtime never writes to AD as part of normal operation. That's the missing tier in our model — we've been smearing directory concerns across the bus and store. Pulling them into a *directory* is the architectural move.
2. **AD treats people, machines, and service accounts as siblings in one schema.** Different object types, same query surface. Exactly the person/agent/device picture, made operational.
3. **AD federates.** Forests, domains, trust relationships. Cross-fleet collaboration is federation, not a shared global directory.
4. **AD is read-mostly from its consumers.** Mirrors OpenStory's read-only-on-agent posture, recursively: the directory is itself read-only from most of OpenStory; only the person writes to it.

### The corrected mint rule

The earlier "only persons can mint principals" rule still holds — but the scope is OpenStory's directory:

```
on directory.receive(Edit { from, ... }):
    if from is not Person:
        reject("only persons can edit the directory")
```

The agent observed by OpenStory cannot edit the directory that records it. The listener cannot edit it. The bus cannot edit it. Only the person, exercising sovereignty over *their* OpenStory, edits the directory.

This is meta-authorship over the *observation network*, not over the world's agency. The earlier framing was true but mis-scoped. This is the correct scope.

### What this means for the role catalog

The roles in the previous section are roles *within OpenStory's directory*. "Coach" means "this principal, in my OpenStory, has the coach role's permission profile — read-mostly, narrow publish-back." It says nothing about what the underlying agent does in the world; it says only what the agent's *observed activity* is shaped into within OpenStory, and what channels OpenStory routes back to the person.

The biographical-vs-bus-identity distinction also stays — but it's clearer now. The bus identity is a directory record. The biographical identity is the person's felt sense of "this is Bobby" assembled from the observations OpenStory has accumulated. The directory holds records; the person holds meaning.

## Position: embedded directory, federation as a future surface

The earlier section sketched what AD gets us. The pragmatic question — *can we just use AD?* — has a layered answer.

**Decision (v1).** OpenStory ships with an embedded directory. Lives in the data dir alongside the SQLite store. Zero external dependencies. Works for a single person on a laptop, a co-founding pair, a small team — the audience the soul is built for. Requiring AD or any external IdP to use OpenStory would invert the sovereignty promise: your fleet would exist at someone else's pleasure.

**Decision (sustained).** The architecture must keep enterprise federation pluggable. The directory tier is a trait, not a concrete type. The embedded impl is the default; a future external-IdP impl (OIDC, LDAP, Keycloak, AD) must be droppable in without redesign. We make this promise to ourselves *now*, in the abstraction, so we don't get surprised later when an enterprise customer needs it.

**Decision (validation).** We don't get to *claim* pluggability — we have to *demonstrate* it. The validation is a testcontainers spike: define the directory trait, build two backends (embedded + an external IdP via container), and run a BDD conformance suite against both. If both pass the same assertions, the abstraction is real. If either fails, the trait is wrong and we redesign before committing. Same pattern as the existing `EventStore` trait with SQLite + Mongo backends.

**Validation status (2026-05-07): green across four scenarios.** The spike lives at `rs/server/tests/directory_pluggability.rs` (entry) + `rs/server/tests/directory/` (trait, embedded impl, Keycloak impl, conformance). The conformance suite exercises four BDD scenarios — *two persons in one group*, *idempotent operations*, *person in multiple groups*, *lookup misses* — covering all five trait methods plus idempotency, cross-group correctness, and miss-vs-error contract. Both backends pass identically. Run with `cargo test -p open-story-server --test directory_pluggability -- --include-ignored` (Keycloak case requires Docker, ~10s end-to-end after image is cached).

Three integration frictions surfaced and resolved during the spike, all worth recording:

1. **Keycloak 26's `master` realm enforces `sslRequired=external`,** which rejects plain HTTP from a testcontainers-mapped port. Fix: import a custom realm with `sslRequired=none` rather than using `master`.
2. **Keycloak 26's User Profile feature attaches an `UPDATE_PROFILE` required action** to imported users missing firstName/lastName/email, blocking password-grant with "Account is not fully set up". Fix: provide all profile fields (and `temporary: false` on the credential) in the realm import.
3. **Keycloak's User Profile validates `firstName` characters** — strings containing parentheses (e.g. `"Alice (multi)"`) are rejected with `error-person-name-invalid-character`. The embedded backend imposes no such validation. This is a real abstraction tension the trait does not yet resolve: is `display_name` an arbitrary UTF-8 string (embedded behavior) or constrained to "name characters" (Keycloak behavior)? For now we keep test data simple; the resolution is an open question for when we promote out of spike.

All three are exactly what the spike is for — surprises caught in seconds of compute, not weeks of implementation.

### What the embedded directory covers

- Persons (local accounts).
- Principals (registered agents and devices).
- Roles (permission profiles).
- Groups (projects, fleets).
- Trust relationships (federation between OpenStorys — peer-to-peer, no central AD required).
- Policies (retention, visibility, broadcast).
- Audit log of directory edits.

### What an external IdP (someday) covers

- Person identity (login).
- Person-to-person group membership (org structure).
- Federation between persons within the same org.

### What stays with OpenStory regardless of IdP

- Agent and device registration (IdPs don't model these).
- Role permission profiles (OpenStory-specific semantics).
- Observation policies (retention, redaction, broadcast scope).
- Federation between OpenStorys *across* organizations (not the same as IdP federation).

The split is honest: identity is delegable; observation isn't.

## Shipped: PersonId + Fleet View v1 (2026-05-07)

The foundation half of the v1 plan landed in PR #54. What now exists in code:

**Data path.** Every CloudEvent carries `person_id` and `principal_id` extension fields. Stamped at ingest by source-aware callers (the three watcher closures: Claude Code, pi-mono, Hermes) using a pure resolver that maps `(agent, host, user, watch_dir)` → `(person_id, principal_id)` via first-match-wins matchers. Sessions persist their owner's `person_id` and producing `principal_id` on both SQLite and Mongo, with conformance parity proven across backends.

**Configuration.** A `[person]` section in `config.toml` defines the person and their fleet. First boot auto-creates one with detected `(host, user)` matchers and persists it back; idempotent on later boots. Sovereignty stays with the person — the bus rule "only persons edit the directory" is enforced by the fact that nothing else can write to `config.toml`.

**API.** `/api/sessions` response includes `person_id` and `principal_id` per session. New endpoint `GET /api/fleet` returns the configured Person + Principals for the UI to resolve display names.

**UI.** The sidebar groups sessions by `principal_id`, with a header per principal showing its display_name (resolved via `useFleet()`). Sessions without a principal_id land under "Unattributed" pinned last. The biographical-vs-bus-identity distinction from the philosophy comes through: the UI shows your fleet members as named participants, not as opaque rows.

**What this enables in the lived experience.** Open OpenStory and you see your fleet — laptop sessions under one heading, Bobby on Hetzner under another (once registered). The mirror reflects not just *what* happened but *who* did it. This is the smallest visible change that makes the philosophy concrete.

**What's still deferred.** Multi-person mode (sharing, login between humans), OIDC/Keycloak federation (the spike at `rs/server/tests/directory_pluggability.rs` proves we can — v1 doesn't need to), runtime CRUD on principals via API (config-only for v1). The directory trait stays in test scope until a real federation use case surfaces.

## Working notes

(Empty. We fill as we work the questions.)
