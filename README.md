External engine (alpha 2)
=========================

:boom: The alpha is currently broken and will be back soon.

Using engines running outside of the browser for
[analysis on lichess.org](https://lichess.org/analysis).

Installing the official provider
--------------------------------

Provides Stockfish 15 for 64-bit x86 platforms, built with profile-guided
optimization, automatically selecting the best available binary for your CPU.

### Docker & Docker Compose

#### Docker

```sh
$ sh run-docker.sh
```

#### Docker Compose

```sh
$ ./deps.sh
$ docker-compose up
```

Then open the "External engine for Lichess" application or visit
http://localhost:9670/.

### Ubuntu, Debian

```sh
echo 'deb [arch=amd64 trusted=yes] https://lichess-org.github.io/external-engine/debian bullseye main' | sudo tee /etc/apt/sources.list.d/external-engine.list
sudo apt update
sudo apt install remote-uci stockfish
```

Then open the "External engine for Lichess" application or visit
http://localhost:9670/.

### Arch Linux

Install [`remote-uci` from the AUR](https://aur.archlinux.org/packages/remote-uci). Then open the
"External engine for Lichess" application or visit http://localhost:9670/.

### Windows

#### Binary

~~Download the latest installer from the [latest release](https://github.com/lichess-org/external-engine/releases).~~ Coming soon.

#### PowerShell

```sh
$ .\RunDocker.ps1
```

Then visit http://localhost:9670/.

### macOS

We do not provide a ready-made provider at this time.

Third party websites
--------------------

Providers can potentially be opened for use by other chess websites.
Please make an issue or [get in contact](https://discord.gg/lichess) to discuss.

Third party providers
---------------------

> :wrench: :hammer: The protocol is subject to change.
> Please make an issue or [get in contact](https://discord.gg/lichess) to discuss.

Lichess provides a reference implementation for an external engine provider.
Third parties can also
[implement their own engine providers](https://lichess.org/api#tag/External-engine-(draft)).

### Engine requirements

To properly work on the Lichess analysis board, engines must support:

* `UCI_Chess960` (enable always!)
* `MultiPV`
* `info` with
  - `depth`
  - `multipv` (between 1 and 5)
  - `score`
  - `nodes`
  - `time`
  - `pv`
