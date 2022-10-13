External engine (alpha 2)
=========================

:boom: The alpha is currently broken and will be back soon.

Using engines running outside of the browser for
[analysis on lichess.org](https://lichess.org/analysis).

Example provider
----------------

1. Create a token at https://lichess.org/account/oauth/token/create?scopes[]=engine:read&scopes[]=engine:write

2. Run:

   ```
   LICHESS_API_TOKEN=lip_*** python3 example-provider.py --engine /usr/bin/stockfish
   ```

3. Visit https://lichess.org/analysis

4. Open the hamburger menu and select the *Example* provider

Official provider
-----------------

An official (more user-friendly) provider is under development.

Will provide Stockfish 15 for 64-bit x86 platforms, built with profile-guided
optimization, automatically selecting the best available binary for your CPU.

Third party providers
---------------------

> :wrench: :hammer: The protocol is subject to change.
> Please make an issue or [get in contact](https://discord.gg/lichess) to discuss.

Lichess provides a reference implementation for an external engine provider.
Third parties can also
[implement their own engine providers](https://lichess.org/api#tag/External-engine-(draft)).
