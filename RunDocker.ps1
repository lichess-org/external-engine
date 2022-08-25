Invoke-Expression -Command $PSScriptRoot\Deps.ps1

docker build . --tag external-engine
docker run -it -p 127.0.0.1:9670:9670 external-engine
