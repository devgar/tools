//! AT Protocol client for Bluesky (bsky.social PDS).
//!
//! Encapsulates the full XRPC protocol: URL construction, session management,
//! JWT TTL, and all HTTP calls. `lib.rs` only touches business types.

use std::sync::Arc;

use postkit_core::{TokenSet, TokenSink};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

const PDS: &str = "https://bsky.social";
const ACCESS_JWT_TTL: std::time::Duration = std::time::Duration::from_secs(45 * 60);
// Conservative TTL for Bluesky's refresh JWT (~90 real days; using 80 for safety margin).
const REFRESH_JWT_TTL_SECS: i64 = 80 * 24 * 3600;

#[derive(Deserialize)]
struct SessionResponse {
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: String,
    did: String,
    handle: String,
}

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) access_jwt: String,
    refresh_jwt: String,
    pub(crate) did: String,
    pub(crate) handle: String,
    created_at: std::time::Instant,
}

impl From<SessionResponse> for Session {
    fn from(r: SessionResponse) -> Self {
        Session {
            access_jwt: r.access_jwt,
            refresh_jwt: r.refresh_jwt,
            did: r.did,
            handle: r.handle,
            created_at: std::time::Instant::now(),
        }
    }
}

pub(crate) struct CreateRecordResult {
    pub(crate) uri: String,
    pub(crate) author_handle: String,
    pub(crate) raw: Value,
}

pub(crate) struct BskyClient {
    http: Client,
    session: Arc<RwLock<Option<Session>>>,
    account_id: String,
    handle: String,
    password: String,
    token_sink: Option<Arc<dyn TokenSink>>,
}

impl BskyClient {
    pub(crate) fn new(account_id: String, handle: String, password: String) -> Self {
        Self {
            http: Client::new(),
            session: Arc::new(RwLock::new(None)),
            account_id,
            handle,
            password,
            token_sink: None,
        }
    }

    pub(crate) fn with_token_sink(mut self, sink: Arc<dyn TokenSink>) -> Self {
        self.token_sink = Some(sink);
        self
    }

    fn persist_session(&self, session: &Session) {
        if let Some(ref sink) = self.token_sink {
            let sink = sink.clone();
            let account_id = self.account_id.clone();
            let tokens = TokenSet {
                access_token: session.access_jwt.clone(),
                refresh_token: Some(session.refresh_jwt.clone()),
                expires_at: Some(chrono::Utc::now().timestamp() + REFRESH_JWT_TTL_SECS),
            };
            tokio::spawn(async move {
                let _ = sink.save(&account_id, &tokens).await;
            });
        }
    }

    // ─── Generic XRPC ────────────────────────────────────────────────────────

    async fn xrpc_post<T: DeserializeOwned>(
        &self,
        nsid: &str,
        auth: Option<&str>,
        body: Option<&Value>,
    ) -> anyhow::Result<T> {
        let mut req = self.http.post(format!("{PDS}/xrpc/{nsid}"));
        if let Some(token) = auth {
            req = req.bearer_auth(token);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        Ok(req.send().await?.error_for_status()?.json().await?)
    }

    async fn xrpc_get<T: DeserializeOwned>(
        &self,
        nsid: &str,
        auth: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(format!("{PDS}/xrpc/{nsid}"))
            .bearer_auth(auth)
            .query(params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    // ─── Session ─────────────────────────────────────────────────────────────

    pub(crate) async fn ensure_session(&self) -> anyhow::Result<Session> {
        {
            let guard = self.session.read().await;
            if let Some(s) = guard.as_ref() {
                if s.created_at.elapsed() < ACCESS_JWT_TTL {
                    return Ok(s.clone());
                }
            }
        }

        let mut guard = self.session.write().await;
        if let Some(s) = guard.as_ref() {
            if s.created_at.elapsed() < ACCESS_JWT_TTL {
                return Ok(s.clone());
            }
            let res: SessionResponse = self
                .xrpc_post("com.atproto.server.refreshSession", Some(&s.refresh_jwt.clone()), None)
                .await?;
            let session = Session::from(res);
            self.persist_session(&session);
            *guard = Some(session.clone());
            return Ok(session);
        }

        // No in-memory session: try with the refresh_jwt stored in the sink.
        if let Some(ref sink) = self.token_sink {
            if let Ok(Some(stored)) = sink.load(&self.account_id).await {
                if let Some(ref refresh_jwt) = stored.refresh_token {
                    match self
                        .xrpc_post::<SessionResponse>(
                            "com.atproto.server.refreshSession",
                            Some(refresh_jwt),
                            None,
                        )
                        .await
                    {
                        Ok(res) => {
                            let session = Session::from(res);
                            self.persist_session(&session);
                            *guard = Some(session.clone());
                            return Ok(session);
                        }
                        Err(e) => {
                            tracing::debug!(
                                account_id = %self.account_id,
                                "stored refresh_jwt expired, re-authenticating: {e}"
                            );
                        }
                    }
                }
            }
        }

        let res: SessionResponse = self
            .xrpc_post(
                "com.atproto.server.createSession",
                None,
                Some(&json!({ "identifier": self.handle, "password": self.password })),
            )
            .await?;
        let session = Session::from(res);
        self.persist_session(&session);
        *guard = Some(session.clone());
        Ok(session)
    }

    // ─── API operations ───────────────────────────────────────────────────────

    /// Uploads raw bytes and returns the blob object to embed in a record.
    /// Uses a direct HTTP request (not a JSON body), so it bypasses xrpc_post.
    pub(crate) async fn upload_blob(&self, bytes: Vec<u8>, mime: &str) -> anyhow::Result<Value> {
        #[derive(Deserialize)]
        struct Res {
            blob: Value,
        }
        let s = self.ensure_session().await?;
        let res: Res = self
            .http
            .post(format!("{PDS}/xrpc/com.atproto.repo.uploadBlob"))
            .bearer_auth(&s.access_jwt)
            .header("Content-Type", mime)
            .body(bytes)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res.blob)
    }

    pub(crate) async fn resolve_handle(&self, handle: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Res {
            did: String,
        }
        let s = self.ensure_session().await?;
        let res: Res = self
            .xrpc_get("com.atproto.identity.resolveHandle", &s.access_jwt, &[("handle", handle)])
            .await?;
        Ok(res.did)
    }

    /// Replaces `_pending_mention` facets in-place with resolved AT mentions.
    /// Handles that cannot be resolved are silently discarded.
    pub(crate) async fn resolve_mentions(&self, facets: &mut Value) {
        let Some(arr) = facets.as_array_mut() else {
            return;
        };
        for facet in arr.iter_mut() {
            let Some(features) = facet["features"].as_array_mut() else {
                continue;
            };
            for feature in features.iter_mut() {
                if feature["$type"] != "_pending_mention" {
                    continue;
                }
                let Some(handle) = feature["handle"].as_str() else {
                    continue;
                };
                let handle = handle.to_string();
                let Ok(did) = self.resolve_handle(&handle).await else {
                    continue;
                };
                *feature = json!({ "$type": "app.bsky.richtext.facet#mention", "did": did });
            }
        }
    }

    pub(crate) async fn create_record(&self, record: Value) -> anyhow::Result<CreateRecordResult> {
        let s = self.ensure_session().await?;
        let raw: Value = self
            .xrpc_post(
                "com.atproto.repo.createRecord",
                Some(&s.access_jwt),
                Some(&json!({
                    "repo": s.did,
                    "collection": "app.bsky.feed.post",
                    "record": record,
                })),
            )
            .await?;
        Ok(CreateRecordResult {
            uri: raw["uri"].as_str().unwrap_or_default().to_string(),
            author_handle: s.handle,
            raw,
        })
    }
}
