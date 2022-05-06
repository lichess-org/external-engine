External engine
===============

Protocol
--------

## Overview

The external engine provider is a WebSocket server. To inform the client about
the connection details, it triggers a navigation to an authorization endpoint,
where the user can confirm that their client should use the given engine
provider. The client will then open a WebSocket connection for each session
with a chess engine.

The client sends [UCI commands](https://backscattering.de/chess/uci/#gui)
as text messages over the WebSocket connection. Each command is
sent in its own WebSocket message, containing no line feeds or carriage
returns.

The provider responds as if the client were exclusively communicating with
an UCI engine, by sending
[UCI commands](https://backscattering.de/chess/uci/#engine) as individual
WebSocket messages. `copyprotection` and `registration` are ignored.

## Important considerations for providers

The most straight-forward implementation would be to forward all WebSocket
messages to an UCI engine as a thin proxy. However, some important
considerations arise.
