External engine
===============

Protocol
--------

## Overview

Lichess provides a reference implementation for an external engine provider.
Third parties can also implement their own engine providers.

An external engine provider is a WebSocket server. To inform the client about
the connection details, it triggers a navigation to an authorization endpoint,
where the user can confirm that their client should use the given engine
provider. The client will then open a WebSocket connection for each session
with a chess engine.

The client sends [UCI commands](https://backscattering.de/chess/uci/#gui)
as text messages over the WebSocket connection. Each command is
sent in its own WebSocket message, containing no line feeds or carriage
returns.

The provider responds as if the client were exclusively communicating with
a UCI engine, by sending
[UCI commands](https://backscattering.de/chess/uci/#engine) as individual
WebSocket messages. `copyprotection` and `registration` are not supported.

## Important considerations for providers

The most straight-forward implementation would be to forward all WebSocket
messages to a UCI engine as a thin proxy. However, some important
considerations arise that require dealing with UCI specifics and tracking
the engine state.

* :warning: With many UCI engines, a malicious user who can execute arbitrary
  commands will be able to damage the host system, cause data loss,
  exfiltrate data, or even achieve arbitrary code execution.

  Recommendation: Use the `safe-uci` adapter (TODO) as a wrapper
  around the engine. If possible, bind the server only on the loopback
  interface to limit the attack surface (TODO: more obvious secret).

* Network connections can be interrupted.

  Recommendation: Send pings over all WebSocket connections at intervals.
  If a client times out or disconnects, stop ongoing searches in order to
  prevent deep or infinite analysis from consuming resources indefinitely.

* Clients may open multiple connections.

  Recommendation: Manage shared access to a single engine process.
  At each point, one of the WebSocket connections has an exclusive session with
  the engine. Track the engine state and options associated with each session.

  When receiving a message (except `stop`) on a connection, an
  exclusive session is requested for that connection. In order to switch
  sessions, end any ongoing search in the previous session
  (by injecting `stop`) and wait until any outstanding engine output has been
  delivered. Then issue `ucinewgame`, to ensure the following session is clean,
  and reapply any options associated with the session (TODO).
