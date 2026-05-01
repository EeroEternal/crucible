//! crucible-x-r2 — SDK core
//!
//! Fetches X.com bookmarks via the v2 API (OAuth 1.0a) and uploads each
//! tweet's raw JSON to a Cloudflare R2 bucket (S3-compatible).

use anyhow::{Context, Result};
use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::{config::Credentials, primitives::ByteStream, Client as S3Client};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use rand::Rng;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ── X API response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct Tweet {
    pub id: String,
    pub text: String,
    // Any extra fields the API returns are captured here so nothing is lost.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BookmarksResponse {
    data: Option<Vec<Tweet>>,
    meta: Option<Meta>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    next_token: Option<String>,
}

// ── OAuth 1.0a helper ─────────────────────────────────────────────────────────

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            other => {
                out.push('%');
                out.push_str(&format!("{:02X}", other));
            }
        }
    }
    out
}

/// Build the `Authorization: OAuth …` header value for a GET request.
///
/// * `method`         — HTTP verb (e.g. "GET")
/// * `url`            — full URL without query string
/// * `query_params`   — query-string key/value pairs
/// * `consumer_key`   — OAuth consumer key (X API key)
/// * `consumer_secret`— OAuth consumer secret (X API secret)
/// * `token`          — OAuth access token
/// * `token_secret`   — OAuth access token secret
fn oauth1_header(
    method: &str,
    url: &str,
    query_params: &BTreeMap<&str, &str>,
    consumer_key: &str,
    consumer_secret: &str,
    token: &str,
    token_secret: &str,
) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let nonce: String = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    // Collect all parameters (OAuth + query string) for signature base.
    let mut params: BTreeMap<&str, String> = BTreeMap::new();
    params.insert("oauth_consumer_key", consumer_key.to_string());
    params.insert("oauth_nonce", nonce.clone());
    params.insert("oauth_signature_method", "HMAC-SHA1".to_string());
    params.insert("oauth_timestamp", timestamp.clone());
    params.insert("oauth_token", token.to_string());
    params.insert("oauth_version", "1.0".to_string());
    for (k, v) in query_params {
        params.insert(k, v.to_string());
    }

    // Build signature base string.
    let param_string = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let base_string = format!(
        "{}&{}&{}",
        percent_encode(method),
        percent_encode(url),
        percent_encode(&param_string)
    );

    // Build signing key.
    let signing_key = format!(
        "{}&{}",
        percent_encode(consumer_secret),
        percent_encode(token_secret)
    );

    // HMAC-SHA1 signature.
    let mut mac = Hmac::<Sha1>::new_from_slice(signing_key.as_bytes())
        .expect("HMAC can accept any key length");
    mac.update(base_string.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());

    // Build Authorization header (only OAuth params, not query params).
    let mut oauth_params = vec![
        format!("oauth_consumer_key=\"{}\"", percent_encode(consumer_key)),
        format!("oauth_nonce=\"{}\"", percent_encode(&nonce)),
        format!("oauth_signature=\"{}\"", percent_encode(&signature)),
        format!("oauth_signature_method=\"HMAC-SHA1\""),
        format!("oauth_timestamp=\"{}\"", percent_encode(&timestamp)),
        format!("oauth_token=\"{}\"", percent_encode(token)),
        format!("oauth_version=\"1.0\""),
    ];
    oauth_params.sort();

    format!("OAuth {}", oauth_params.join(", "))
}

// ── Main SDK struct ───────────────────────────────────────────────────────────

/// Configuration for the sync client.
pub struct CrucibleXSync {
    http: HttpClient,
    r2_client: S3Client,
    bucket: String,
    user_id: String,
    api_key: String,
    api_secret: String,
    access_token: String,
    access_secret: String,
}

impl CrucibleXSync {
    /// Create a new sync client.
    ///
    /// # Arguments
    /// * `api_key` / `api_secret`         — X (Twitter) app credentials
    /// * `access_token` / `access_secret` — X user OAuth 1.0a tokens
    /// * `r2_account_id`                  — Cloudflare account ID
    /// * `r2_access_key` / `r2_secret_key`— R2 API token credentials
    /// * `bucket`                         — R2 bucket name
    /// * `user_id`                        — Numeric X user ID whose bookmarks to fetch
    pub async fn new(
        api_key: &str,
        api_secret: &str,
        access_token: &str,
        access_secret: &str,
        r2_account_id: &str,
        r2_access_key: &str,
        r2_secret_key: &str,
        bucket: &str,
        user_id: &str,
    ) -> Result<Self> {
        let endpoint = format!(
            "https://{}.r2.cloudflarestorage.com",
            r2_account_id
        );

        let r2_config = aws_config::defaults(BehaviorVersion::latest())
            .endpoint_url(&endpoint)
            .credentials_provider(Credentials::new(
                r2_access_key,
                r2_secret_key,
                None,
                None,
                "r2",
            ))
            .region(Region::new("auto"))
            .load()
            .await;

        let r2_client = S3Client::new(&r2_config);
        let http = HttpClient::new();

        Ok(Self {
            http,
            r2_client,
            bucket: bucket.to_string(),
            user_id: user_id.to_string(),
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            access_token: access_token.to_string(),
            access_secret: access_secret.to_string(),
        })
    }

    /// Fetch a single page of bookmarks from the X v2 API.
    async fn fetch_bookmark_page(
        &self,
        pagination_token: Option<&str>,
    ) -> Result<BookmarksResponse> {
        let url = format!(
            "https://api.twitter.com/2/users/{}/bookmarks",
            self.user_id
        );

        let mut query: BTreeMap<&str, &str> = BTreeMap::new();
        query.insert("max_results", "100");
        // Request extra tweet fields so the stored JSON is richer.
        query.insert(
            "tweet.fields",
            "attachments,author_id,created_at,entities,geo,id,lang,\
             possibly_sensitive,public_metrics,referenced_tweets,source,text",
        );

        let token_holder;
        if let Some(token) = pagination_token {
            token_holder = token.to_string();
            query.insert("pagination_token", &token_holder);
        }

        let auth = oauth1_header(
            "GET",
            &url,
            &query,
            &self.api_key,
            &self.api_secret,
            &self.access_token,
            &self.access_secret,
        );

        let mut req = self.http.get(&url).header("Authorization", auth);
        for (k, v) in &query {
            req = req.query(&[(k, v)]);
        }

        let resp = req
            .send()
            .await
            .context("Failed to call X bookmarks API")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("X API error {}: {}", status, body);
        }

        resp.json::<BookmarksResponse>()
            .await
            .context("Failed to parse X bookmarks response")
    }

    /// Upload a single tweet's JSON to R2.
    async fn upload_tweet(&self, tweet: &Tweet) -> Result<()> {
        let key = format!("bookmarks/{}/{}.json", self.user_id, tweet.id);
        let body = serde_json::to_vec(tweet).context("Failed to serialise tweet")?;

        self.r2_client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(body))
            .content_type("application/json")
            .send()
            .await
            .with_context(|| format!("Failed to upload tweet {} to R2", tweet.id))?;

        Ok(())
    }

    /// Sync **all** bookmarks to R2, following pagination automatically.
    ///
    /// Returns the total number of tweets uploaded.
    pub async fn sync_bookmarks(&self) -> Result<u32> {
        let mut count = 0u32;
        let mut next_token: Option<String> = None;

        loop {
            let resp = self
                .fetch_bookmark_page(next_token.as_deref())
                .await?;

            let tweets = resp.data.unwrap_or_default();

            for tweet in &tweets {
                self.upload_tweet(tweet).await?;
                count += 1;
                println!("Uploaded: {}", tweet.id);
            }

            next_token = resp.meta.and_then(|m| m.next_token);
            if next_token.is_none() {
                break;
            }
        }

        Ok(count)
    }

    /// Sync only the first `limit` bookmarks to R2.
    pub async fn sync_bookmarks_limit(&self, limit: u32) -> Result<u32> {
        let mut count = 0u32;
        let mut next_token: Option<String> = None;

        'outer: loop {
            let resp = self
                .fetch_bookmark_page(next_token.as_deref())
                .await?;

            let tweets = resp.data.unwrap_or_default();

            for tweet in &tweets {
                if count >= limit {
                    break 'outer;
                }
                self.upload_tweet(tweet).await?;
                count += 1;
                println!("Uploaded: {}", tweet.id);
            }

            next_token = resp.meta.and_then(|m| m.next_token);
            if next_token.is_none() {
                break;
            }
        }

        Ok(count)
    }
}
