External engine (alpha 2)
=========================

Using engines running outside of the browser for
[analysis on lichess.org](https://lichess.org/analysis).

Example provider
----------------

1. Create a token at https://lichess.org/account/oauth/token/create?scopes[]=engine:read&scopes[]=engine:write

2. Install your favorite engine. 
   (Under Windows is it is recommended to place it directly in ```C```. Under Unix choose ```usr/bin/YOUR_ENGINE_FOLDER/program```)

3. Install python3 in case you have not: https://www.python.org/downloads/

4. Install pip3 - Packages 

   First go into your project folder.

   On Windows: 

    ```
      py -m ensurepip --upgrade
      py -m pip install -r requirements.txt 
    ```

   On other systems:

    ```
      py3 -m ensurepip --upgrade
      py3 -m pip install -r requirements.txt 
    ```

3. 

   On Windows run:

   ```
   py3 example-provider.py --engine C:\YOUR_ENGINE_FOLDER\program.exe --token YOUR_TOKEN
   ```

   On Unix run:

   ```
   LICHESS_API_TOKEN=YOUR_TOKEN python3 example-provider.py --engine /usr/bin/YOUR_ENGINE_FOLDER/program
   ```

   The connector should now run successful. 

3. Visit https://lichess.org/analysis

4. Open the hamburger menu and select the *Alpha 2* provider

Official provider
-----------------

An official (more user-friendly) provider is under development.

Will provide Stockfish 15 for 64-bit x86 platforms, built with profile-guided
optimization, automatically selecting the best available binary for your CPU.

Third party clients and providers
---------------------------------

> :wrench: :hammer: The protocol is subject to change.
> Please make an issue or [get in contact](https://discord.gg/lichess) to discuss.

Lichess provides an
[HTTP API for third-party clients and providers](https://lichess.org/api#tag/External-engine).



