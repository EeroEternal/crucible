# crucible-x-r2

A Rust SDK + CLI that fetches your **X.com bookmarks** (Twitter v2 API) and
stores each tweet as raw JSON in a **Cloudflare R2** bucket.

No web UI, no login flow — just a personal script/library driven by environment
variables and OAuth 1.0a tokens.

---

## Features

- Paginated bookmark sync (follows `next_token` automatically)
- Optional `--limit N` to sync only the most recent N bookmarks
- Each tweet is stored at `bookmarks/<user_id>/<tweet_id>.json`
- Usable both as a **CLI binary** and as a **Rust library**
- OAuth 1.0a signing implemented with `hmac` + `sha1` — no heavy auth framework needed

---

## Requirements

| Tool | Version |
|------|---------|
| Rust | 1.75+   |

---

## Setup

### 1. Create an X developer app

1. Go to <https://developer.twitter.com> and create a project + app.
2. Enable **OAuth 1.0a** and set permissions to **Read** (bookmarks are read-only).
3. Generate an **Access Token** and **Access Token Secret** for your own account.

### 2. Create a Cloudflare R2 bucket

1. Log in to the Cloudflare dashboard → **R2**.
2. Create a bucket (e.g. `crucible-bookmarks`).
3. Under **Manage R2 API Tokens**, create a token with **Object Read & Write** on
   that bucket.

### 3. Configure environment variables

```bash
cp .env.example .env
# Fill in the values in .env
```

`.env` keys:

| Key | Description |
|-----|-------------|
| `X_API_KEY` | X app API key |
| `X_API_SECRET` | X app API secret |
| `X_ACCESS_TOKEN` | OAuth 1.0a access token |
| `X_ACCESS_SECRET` | OAuth 1.0a access token secret |
| `R2_ACCOUNT_ID` | Cloudflare account ID |
| `R2_ACCESS_KEY_ID` | R2 API token access key |
| `R2_SECRET_ACCESS_KEY` | R2 API token secret key |
| `R2_BUCKET_NAME` | R2 bucket name |

---

## Usage

### Build

```bash
cd crucible-x-r2
cargo build --release
```

### CLI — sync all bookmarks

```bash
./target/release/crucible-x-r2 --user-id 1234567890
```

### CLI — sync only the most recent 50 bookmarks

```bash
./target/release/crucible-x-r2 --user-id 1234567890 --limit 50
```

### As a Rust library

Add to your `Cargo.toml`:

```toml
[dependencies]
crucible-x-r2 = { path = "../crucible-x-r2" }
```

Then:

```rust
use crucible_x_r2::CrucibleXSync;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let syncer = CrucibleXSync::new(
        &std::env::var("X_API_KEY")?,
        &std::env::var("X_API_SECRET")?,
        &std::env::var("X_ACCESS_TOKEN")?,
        &std::env::var("X_ACCESS_SECRET")?,
        &std::env::var("R2_ACCOUNT_ID")?,
        &std::env::var("R2_ACCESS_KEY_ID")?,
        &std::env::var("R2_SECRET_ACCESS_KEY")?,
        &std::env::var("R2_BUCKET_NAME")?,
        "1234567890",
    )
    .await?;

    let count = syncer.sync_bookmarks().await?;
    println!("Synced {} bookmark(s)", count);
    Ok(())
}
```

---

## R2 object layout

```
crucible-bookmarks/
└── bookmarks/
    └── <user_id>/
        ├── <tweet_id_1>.json
        ├── <tweet_id_2>.json
        └── ...
```

Each JSON file contains the full tweet object returned by the X v2 API,
including `attachments`, `author_id`, `created_at`, `entities`, `geo`,
`public_metrics`, and more.

---

## License

MIT
