# hopper-server

Hopper's website: the page, and a small JSON API.

- `GET /`: the page.
- `GET /api/check?mod=<Modrinth link or slug>&game=<Minecraft version>`: the checker's report, as JSON. `game` is optional; without it the newest Fabric version is checked.
- `GET /api/port?mod=...&game=...`: a zip of the starting Horizon project the generator writes (the page's Download starter project button). Refused for licenses that stop porting.
- `GET /api/library`: how many mappings the library has.

## Caching

Each mod version is downloaded and checked once, not once per visitor:

- What Modrinth says a link is (the project and its newest version) is kept for 10 minutes. A new version of a mod shows within that time.
- Reports and project zips are kept on disk per mod version, in `--cache <dir>` (default `./cache`), and the most recent 500 in memory too. A restart keeps them.
- Results are tied to the library and to this server's program: new mappings or a new Hopper give fresh results, and the old ones are deleted at start. Files nobody asked for in 30 days are deleted at start too.
- When many people ask for the same new result at once, one request does the work and the others wait for it.
- Every answer says `X-Hopper-Cache: hit` (kept) or `miss` (made now).

`--no-cache` keeps results in memory only. At most 4 downloads run at once, and jars over 50 MB aren't downloaded.

## Running it

```sh
cargo build --release -p hopper-server
./target/release/hopper-server --listen 127.0.0.1:8080 --library ./library --cache ./cache
```

It serves plain HTTP on the address given. On a server, put it behind a reverse proxy that adds HTTPS, such as [Caddy](https://caddyserver.com):

```
hopper.example {
    reverse_proxy 127.0.0.1:8080
}
```

The library is read once at start: restart the server after pulling new entries.
