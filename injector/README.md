# injector

A box must reach GitHub and its tracker, but it should not hold a token that works anywhere
else. The box holds a placeholder (`tbxph_...`) instead. The injector sits behind the egress
proxy and writes the real value into each request on its way out.

One static Rust binary on `scratch`, about 4 MB.

## How a request goes

```
box ── HTTPS_PROXY ──▶ Squid ──▶ the internet          every other host
                         │
                         └─ CONNECT <credential host> ──▶ injector ──TLS──▶ the host
                                                              │
                                                              └─ placeholder? ask the tenbox server
```

1. Squid sends only the CONNECT of a credential host (`--host`, e.g. `github.com`,
   `api.github.com`) to the injector; the injector refuses any other host or port.
2. The injector ends the box's TLS with a leaf of its own CA. The leaf names that one host and
   lives 48 hours; the injector issues it again a day before its end. The box trusts the CA
   through `SSL_CERT_FILE`, `GIT_SSL_CAINFO` and `NODE_EXTRA_CA_CERTS`; the CA's name
   constraints allow only the credential hosts, so its key cannot forge any other site.
3. When `Authorization` holds a placeholder (git's Basic password, `token`, `Bearer` or the
   bare value of a tracker key), the injector asks the tenbox server for the real value of that
   placeholder and host, keeps it for 60 seconds, and writes it in. It never reads or changes
   a body. A placeholder the server does not know is a 502 for the box.
4. The request goes on over TLS to the real host, checked against the Mozilla roots
   (`webpki-roots`).

## Code

| File | Does |
|---|---|
| `src/main.rs` | the options and the accept loop |
| `src/tunnel.rs` | one box connection: the CONNECT, the TLS, each request on to the host |
| `src/certs.rs` | the leaf of each host, signed by the CA |
| `src/credentials.rs` | finds the placeholder in `Authorization` and writes the real value |
| `src/server.rs` | asks the tenbox server `POST <--server>` `{ placeholder, host }` → `{ value }` |

```
injector --ca-cert <pem> --ca-key <pem> --host <name> [--host <name>...]
         --server http://<host>:<port>/<path> --server-token <file> [--listen 0.0.0.0:3129]
```

## Build and test

```sh
docker build --target check injector   # cargo fmt, clippy with -D warnings, the tests
docker build injector -t injector      # the image
```

`.github/workflows/injector.yml` runs both on every change to `injector/` and pushes the image
to `ghcr.io/enojen/tenyard-injector` as `:<commit>`, on `main` also as `:latest`.
