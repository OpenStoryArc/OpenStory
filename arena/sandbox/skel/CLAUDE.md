# Working in this sandbox

Orientation — read before probing; it saves a dozen tool calls.

## Constraints that will bite you
1. You cannot install anything: root FS is read-only, no sudo, no internet egress.
   `npm install`, `pip install`, `apt-get`, and `git clone` from public hosts fail.
2. Only Node's standard library is available (Node 22). Build with `node:*` built-ins.
3. `/tmp` is small and `noexec` — put scratch scripts in `~/.scratch` (exec-capable).

## Network
No public internet. The internal arena network works: `arena-litellm:4000` serves the
model. WebFetch/WebSearch are unavailable.

## OpenStory (your history)
OpenStory runs in this container and records every agent session here. Data:
`~/data/open-story.db`. An `openstory` MCP server for reading this history from inside the
box is being added — until it lands, use the dashboard (the `-story` link) to read your
history.

## Repo
`~/workspace` is a git repo; a global git identity is already configured.
