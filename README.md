<div align="center">

<img src="assets/icon.svg" alt="openpaste" width="96" height="96">

# openpaste

**A paste service for text, terminal output and binaries — one Rust binary, one optional web UI.**

[![License](https://img.shields.io/github/license/Im-Fran/openpaste)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-000?logo=rust)](Cargo.toml)

</div>

```
  +--------------------------------+
  |  o  o  o                       |
  |                                |
  |   $ echo 'hi' | openpaste up   |
  |   > /paste/x7Kf2a9Q _          |
  |                                |
  +--------------------------------+
            o p e n p a s t e
```

---

## 📖 Overview

openpaste is a self-hosted pastebin that treats the terminal as a first-class client. You can
paste code in the browser, pipe the output of a command straight into it with `curl`, or upload
a binary and hand someone a download link — all against the same endpoint.

The backend is a single Rust binary built on **axum** and **sqlx**. Text pastes live in the
database; anything that isn't valid UTF-8 is stored as a blob on the local filesystem or in S3.
The database is chosen at runtime from `DATABASE_URL`, so the same binary runs on SQLite for a
personal instance and on PostgreSQL for a shared one.

The frontend is **server-rendered HTML with [htmx](https://htmx.org)**. The templates and their
CSS live in `assets/web/` as plain files and are embedded into the binary at compile time, so
there is no build step and no Node.js — edit the HTML, rebuild, done. If you don't want a UI at
all, run `--headless` (or build without the `frontend` feature) and the service answers plain
text to `curl` and nothing else.

---

## ✨ Features

- **Share by URL** — every paste is available at `/paste/:id`, and `/paste/:id/raw` returns the
  exact bytes with no HTML around them.
- **Pipe from the terminal** — `echo 'test' | curl --data-binary @- https://example.com` returns
  the URL on stdout, ready to be copied or piped further.
- **Binary uploads** — non-UTF-8 content is detected automatically and served from
  `/paste/:id/download` as an attachment, with the original filename and a guessed content type.
- **Pluggable storage** — blobs go to the local filesystem or to any S3-compatible bucket
  (AWS, MinIO, R2), selected with `STORAGE_DRIVER`.
- **Pluggable database** — SQLite or PostgreSQL, selected by the scheme in `DATABASE_URL`.
- **Configurable size limit** — `MAX_UPLOAD_BYTES` (100 MiB by default) is enforced on the
  request body, so oversized uploads are rejected with `413` before being buffered.
- **Headless mode** — API-only operation for servers with no web UI.
- **Built-in CLI** — the same binary is also a client: `openpaste up` and `openpaste get`.
- **Runs anywhere** — Docker image, Docker Compose stack, or a systemd unit on a plain VPS.

---

## 🛠 Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust 2021, [axum](https://github.com/tokio-rs/axum) 0.8, tokio |
| Frontend | Server-rendered HTML + htmx 2 (`assets/web/`, embedded via `rust-embed`) |
| Database | SQLite or PostgreSQL, via `sqlx` `Any` driver |
| Blob storage | Local filesystem or S3, via `object_store` |
| CLI | `clap` 4 + `reqwest` |
| Packaging | Docker, Docker Compose, systemd |

---

## 📋 Requirements

- **Rust** 1.80+ (`cargo`) — tested on 1.98
- **Git**
- Optional: **Docker** 24+ with Compose v2, or **PostgreSQL** 14+ for a non-SQLite instance

---

## 🚀 Getting Started

### 1. Clone the repository

```bash
git clone https://github.com/Im-Fran/openpaste.git
cd openpaste
```

### 2. Configure the environment

```bash
cp .env.example .env
```

| Variable | Description | Default |
|----------|-------------|---------|
| `BIND` | Address the server listens on | `0.0.0.0:8080` |
| `BASE_URL` | Public URL used to build the links handed back to clients | `http://localhost:8080` |
| `DATABASE_URL` | `sqlite://...` or `postgres://...` | `sqlite://./data/openpaste.db?mode=rwc` |
| `STORAGE_DRIVER` | Where binaries are stored: `local` or `s3` | `local` |
| `STORAGE_PATH` | Blob directory when `STORAGE_DRIVER=local` | `./data/blobs` |
| `S3_BUCKET` | Bucket name — required when `STORAGE_DRIVER=s3` | — |
| `S3_PREFIX` | Key prefix inside the bucket | `openpaste` |
| `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT` | Standard AWS credentials; `AWS_ENDPOINT` points at MinIO / R2 / other S3-compatible services | — |
| `MAX_UPLOAD_BYTES` | Maximum upload size in bytes | `104857600` (100 MiB) |
| `HEADLESS` | `true` to disable the web UI | `false` |
| `RUST_LOG` | Log filter | `openpaste=info` |

### 4. Run it

```bash
cargo run -- serve
```

Open [http://localhost:8080](http://localhost:8080).

For frontend work, edit the templates in `assets/web/` (`layout.html`, `new.html`,
`view_text.html`, `view_binary.html`, `style.css`) and re-run `cargo run -- serve`.

---

## 💻 Usage

### From the browser

Paste text into the editor and press **create paste**, or drop a file anywhere on the page.

### From the terminal with `curl`

```bash
# Pipe anything into it; the URL comes back on stdout
echo 'test' | curl --data-binary @- http://localhost:8080

# Send a file, keeping its name (used for the download filename and content type)
curl -T report.pdf http://localhost:8080/report.pdf

# Read it back
curl http://localhost:8080/paste/x7Kf2a9Q/raw
```

A shell function makes it a one-word command:

```bash
# ~/.bashrc or ~/.zshrc
export OPENPASTE_SERVER=https://paste.example.com
paste() { curl -sf --data-binary @- "$OPENPASTE_SERVER"; }

# then:
git diff | paste
journalctl -u nginx -n 200 | paste
```

### With the built-in CLI

```bash
openpaste up report.pdf              # upload a file
git log --oneline | openpaste up     # upload stdin
openpaste get x7Kf2a9Q               # print the raw content to stdout
openpaste get x7Kf2a9Q > out.bin     # binaries stream through unchanged
```

`openpaste up` prints the URL on stdout and, when stderr is a terminal, an ASCII
QR code of that URL on stderr — scan it with a phone. Piping or redirecting keeps
the output clean:

```bash
openpaste up report.pdf | pbcopy   # only the URL, no QR
```

`--server` (or `OPENPASTE_SERVER`) points the client at your instance; it defaults to
`http://localhost:8080`.

### HTTP API

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/` or `/api/pastes` | Create a paste from the raw request body. Optional `X-Filename` header. Returns the URL as plain text, or JSON when `Accept: application/json`. |
| `PUT` | `/:filename` | Same, with the filename taken from the path (this is what `curl -T` does). |
| `GET` | `/paste/:id` | Web UI for the paste (raw content in headless mode). |
| `GET` | `/paste/:id/raw` | Raw content, no attachment header. |
| `GET` | `/paste/:id/download` | Raw content as `Content-Disposition: attachment`. |
| `GET` | `/api/pastes/:id` | Paste metadata as JSON (plus the content, for text pastes). |
| `GET` | `/healthz` | Liveness probe. |

---

## 🏗 Building for Production

```bash
cargo build --release
```

The binary lands in `target/release/openpaste` with the UI embedded — copy that one file to your
server.

To build without the frontend (smaller binary):

```bash
cargo build --release --no-default-features
```

---

## 🌐 Deployment

### Option A: Docker

The image builds the UI, compiles the binary and ships a slim Debian runtime:

```bash
docker build -t openpaste .
docker run -p 8080:8080 -v openpaste-data:/var/lib/openpaste \
  -e BASE_URL=https://paste.example.com openpaste
```

It defaults to SQLite and local blob storage under `/var/lib/openpaste`, so a single volume
keeps everything.

### Option B: Docker Compose (with PostgreSQL)

`docker-compose.yml` brings up openpaste alongside PostgreSQL 17:

```bash
cp .env.example .env   # set BASE_URL and POSTGRES_PASSWORD
docker compose up -d
```

Point `STORAGE_DRIVER=s3` plus the `S3_*` / `AWS_*` variables at a bucket if you don't want the
blobs on the host volume.

### Option C: systemd on a VPS

```bash
sudo useradd -r -d /var/lib/openpaste -m openpaste
sudo install -m755 target/release/openpaste /usr/local/bin/openpaste
sudo install -Dm640 .env.example /etc/openpaste/openpaste.env   # then edit it
sudo install -m644 openpaste.service /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable --now openpaste
sudo systemctl status openpaste
```

The unit runs as an unprivileged `openpaste` user with `ProtectSystem=strict` and only
`/var/lib/openpaste` writable. Put nginx or Caddy in front for TLS, and make sure `BASE_URL`
matches the public hostname — it is what the returned links are built from.

---

## 🧪 Tests

```bash
cargo test          # unit tests
./scripts/smoke.sh  # end-to-end: text, binary, 404, size limit, CLI round-trip
```

The smoke script starts a throwaway server on a temporary SQLite database and checks that a
random 4 KiB blob survives an upload/download round-trip byte for byte.

---

## 🤝 Contributing

Contributions are welcome.

1. Fork the repo
2. Create a branch: `git checkout -b feat/your-feature`
3. Make sure `cargo test` and `./scripts/smoke.sh` pass
4. Commit: `git commit -m "feat: add your feature"`
5. Push and open a PR

---

## 📄 License

MIT — see [LICENSE](LICENSE).

---

<div align="center">
Made with ☕ by <a href="https://franciscosolis.cl">Fran</a>
</div>
