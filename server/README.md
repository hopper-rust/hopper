# hopper-server

Hopper's website: the page, and a small JSON API.

- `GET /`: the page.
- `GET /api/check?mod=<Modrinth link or slug>&game=<Minecraft version>`: the checker's report, as JSON. `game` is optional; without it the newest Fabric version is checked.
- `GET /api/library`: how many mappings the library has.

Reports are kept for an hour, so a mod checked twice is downloaded once. At most 4 checks run at once, and jars over 50 MB aren't downloaded.

## Running it

```sh
cargo build --release -p hopper-server
./target/release/hopper-server --listen 127.0.0.1:8080 --library ./library
```

It serves plain HTTP on the address given. On a server, put it behind a reverse proxy that adds HTTPS, such as [Caddy](https://caddyserver.com):

```
hopper.example {
    reverse_proxy 127.0.0.1:8080
}
```

The library is read once at start: restart the server after pulling new entries.
