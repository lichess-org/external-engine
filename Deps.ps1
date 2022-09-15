git submodule update --init --recursive

Set-Location .\stockfish\vendor

Get-Content downloads.txt |
   Where-Object { -Not (Test-Path $(Split-Path $_ -Leaf)) } |
   ForEach-Object { Invoke-WebRequest $_ -OutFile $(Split-Path $_ -Leaf) }

Set-Location .\..\..
