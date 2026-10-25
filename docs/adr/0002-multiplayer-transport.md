# ADR-0002: Multiplayer transport — Bandit + Phoenix Channels, not LiveView

- Status: Accepted
- Date: 2026-07-09

## Context

IDApTIK is an asymmetric two-player game: an infiltrator and a hacker share
one logical world. Rust owns deterministic game semantics; Elixir/OTP provides
the current multiplayer transport and relay/session coordination. The process
that may commit a session's simulation state is a separate, topology-specific
decision (ADR-0015). The open question here was how the Elixir side should serve
real-time traffic: **Bandit**, **Phoenix LiveView**, or something else, and where
**hex** fits.

These are not alternatives at the same layer, which was the source of confusion:

- **Bandit** is a pure-Elixir HTTP/1.1 + HTTP/2 + WebSocket **server** (a Plug/
  WebSock adapter, the modern replacement for Cowboy). It is *what serves the
  sockets*.
- **Phoenix Channels** is a real-time **messaging layer** (pub/sub, presence,
  topics) that runs *over* a WebSocket served by an adapter like Bandit.
- **Phoenix LiveView** renders rich, server-driven **HTML UI** over a WebSocket.
  It is for web front-ends.
- **hex** is Elixir/Erlang's **package registry** — how `mix` fetches Phoenix,
  Bandit, etc. It is not a component you choose between; it is the plumbing that
  pulls the others.

## Decision

Serve the multiplayer backend with **Bandit** as the HTTP/WebSocket adapter and
**Phoenix Channels** as the real-time transport. **Do not use LiveView** for
gameplay.

Rationale:

- The game UI lives in Rust (Bevy), not in HTML. LiveView's whole value is
  server-rendered HTML DOM diffing — irrelevant to a native/Wasm game client, and
  it would be the wrong abstraction to push game state through.
- Phoenix Channels is the idiomatic BEAM primitive for exactly this: topic-based
  pub/sub, `Phoenix.Presence` for who-is-connected, and built-in reconnection —
  everything a two-player session needs — while leaving the wire payloads to us.
- Bandit is the modern default (it has been Phoenix's default adapter since
  1.7.11), gives us HTTP/2 + WebSockets with no extra work, and keeps the stack
  pure Elixir.
- Phoenix (Channels) buys presence/pub-sub/reconnect for free versus a raw
  `WebSockAdapter` handler on Bandit alone; the framework weight is worth it for
  multiplayer.

## What hex pulls (now in burble's server/mix.exs)

The IDApTIK repository no longer contains Elixir code. The session relay lives
in burble, which uses:

- `{:phoenix, "~> 1.7"}` — Channels, endpoint, socket (Bandit is its default
  adapter; nothing extra to wire for WebSockets)
- `{:bandit, "~> 1.11"}` — the HTTP/WebSocket server
- `{:phoenix_pubsub, "~> 2.1"}` — pub/sub backbone (clustered later if needed)
- `{:jason, "~> 1.4"}` — JSON, until/unless we adopt a binary wire format
- **Not** `{:phoenix_live_view, ...}`

For the burble dependencies, see `metadatastician/burble/server/mix.exs`.

A binary wire format (protobuf/flatbuffers/MessagePack) for the Rust↔Elixir
boundary is deferred to its own ADR once the protocol is designed; Channels are
payload-agnostic, so this decision does not block on it.

## Consequences

- The Elixir app is a headless real-time backend, not a web UI — smaller surface.
- Rust client speaks the Phoenix Channels socket protocol (a documented, stable
  framing) over Bandit's WebSocket.
- If we ever want a browser-based spectator/among-us-style lobby UI, LiveView can
  be added *alongside* for that specific surface without disturbing gameplay.

## Amendment (2026-08-11): burble is the session fabric

As of estate ruling 2026-08-04, the **burble** platform (`metadatastician/burble`) is
the designated gaming communication platform for IDApTIK. The session relay that
was previously implemented in `server/` has been **removed**; all session
relaying now happens through burble's `game:<session_id>` channel (fabric slice
1, burble PR #182).

- The `game:` lane in burble provides the same byte-preserving JSON relay of
  `Command`/`Event` payloads as the former IDApTIK `session_channel.ex`, with the
  addition of the missing `NetSsh`/`NetHack` hacker verbs (fixed in IDApTIK PR #71
  and present in burble's `Burble.Games.Idaptik` profile from inception).
- The client-side transport (`crates/idaptik-net`) now joins `game:<id>` with
  `{"game": "idaptik", "role": "infiltrator" | "hacker"}` params and connects to
  burble's `/voice/websocket` endpoint as a guest.
- burble uses Bandit as its HTTP/WebSocket adapter and Phoenix Channels for
  messaging — the same stack chosen by this ADR; this amendment records the
  **deployment** change, not a transport change.
- The loopback gate (ADR-0006 §4) now uses burble in both IDApTIK and burble
  CI.
