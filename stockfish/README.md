Usage
-----

```sh
wget --no-clobber --input-file downloads.txt
docker build --tag stockfish .
docker run --interactive --tty stockfish
```

Extract binaries
----------------

```sh
id=$(docker create stockfish)
docker cp "$id:/usr/lib/stockfish" dist
```
