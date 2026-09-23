//! A thin Supabase client: GoTrue for auth, PostgREST for tables.
//!
//! Every table request carries the publishable key as `apikey` plus the
//! user's access token as the bearer, so RLS applies exactly as it does in
//! the desktop app. Column names mirror `src/types/database.ts`.

use crate::config::Config;
use crate::session::Session;
use anyhow::{Context, Ok, Result, anyhow, bail};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};

pub struct Supabase {
    http: Client,
    config: Config,
}

// ── Auth payloads ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    user: AuthUser,
}

#[derive(Debug, Deserialize)]
struct AuthUser {
    id: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(alias = "error_description", alias = "msg", alias = "error")]
    message: Option<String>,
    #[serde(default)]
    hint: Option<String>,
    #[serde(default)]
    details: Option<String>,
}

// ── Rows ──────────────────────────────────────────────────────────────────

/// The columns this CLI writes. Everything else takes its schema default
/// (`user_id` defaults to `auth.uid()`, `kind` to `'task'`, timestamps to `now()`).
#[derive(Debug, Serialize)]
pub struct TaskInsert {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub due_date: Option<String>,
    pub priority: Option<String>,
    pub block: Option<String>,
    pub category: Option<String>,
}

impl Supabase {
    pub fn new(config: Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "apikey",
            HeaderValue::from_str(&config.publishable_key).context("invalid publishable key")?,
        );
        let http = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("risebyday-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http, config })
    }

    fn auth_url(&self, path: &str) -> String {
        format!("{}/auth/v1/{path}", self.config.url)
    }

    fn rest_url(&self, table: &str) -> String {
        format!("{}/rest/v1/{table}", self.config.url)
    }

    // ── Auth ────────────────────────────────────────────────────────────

    pub async fn sign_in(&self, email: &str, password: &str) -> Result<Session> {
        let res = self
            .http
            .post(self.auth_url("token?grant_type=password"))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await
            .context("reaching Supabase auth")?;
        let token: TokenResponse = check(res).await?.json().await?;
        Ok(token.into())
    }

    pub async fn refresh(&self, session: &Session) -> Result<Session> {
        let res = self
            .http
            .post(self.auth_url("token?grant_type=refresh_token"))
            .json(&serde_json::json!({ "refresh_token": session.refresh_token }))
            .send()
            .await
            .context("reaching Supabase auth")?;
        let token: TokenResponse = check(res).await?.json().await?;
        Ok(token.into())
    }

    pub async fn sign_out(&self, session: &Session) -> Result<()> {
        // Best effort: a stale token is already useless server-side.
        let _ = self
            .http
            .post(self.auth_url("logout"))
            .bearer_auth(&session.access_token)
            .send()
            .await;
        Ok(())
    }

    /// The saved session, refreshed and re-saved if it is about to expire.
    pub async fn authed(&self) -> Result<Session> {
        let session = Session::load()?.ok_or_else(|| {
            anyhow!("not signed in — run `sign-in --email you@example.com` first")
        })?;
        if !session.needs_refresh() {
            return Ok(session);
        }
        let fresh = self
            .refresh(&session)
            .await
            .context("session expired and could not be refreshed; sign in again")?;
        fresh.save()?;
        Ok(fresh)
    }

    // ── Tasks ───────────────────────────────────────────────────────────

    pub async fn create_task(&self, session: &Session, task: &TaskInsert) -> Result<TaskRow> {
        let res = self
            .table(session, self.http.post(self.rest_url("tasks")))
            .header("Prefer", "return=representation")
            .json(&[task])
            .send()
            .await
            .context("reaching Supabase")?;
        let mut rows: Vec<TaskRow> = check(res).await?.json().await?;
        rows.pop().ok_or_else(|| anyhow!("insert returned no row"))
    }

    /// Open (not done, not deleted) tasks, soonest due first.
    pub async fn list_open_tasks(&self, session: &Session) -> Result<Vec<TaskRow>> {
        let res = self
            .table(session, self.http.get(self.rest_url("tasks")))
            .query(&[
                ("select", "id,title,due_date,priority,block,category"),
                ("deleted_at", "is.null"),
                ("done", "is.false"),
                ("order", "due_date.asc.nullslast,created_at.asc"),
            ])
            .send()
            .await
            .context("reaching Supabase")?;
        Ok(check(res).await?.json().await?)
    }

    pub async fn list_all_tasks(&self, session: &Session) -> Result<Vec<TaskRow>> {
        let res = self
            .table(session, self.http.get(self.rest_url("tasks")))
            .query(&[
                ("select", "id,title,due_date,priority,block,category"),
                ("deleted_at", "is.null"),
            ])
            .send()
            .await
            .context("Reaching Server")?;
        Ok(check(res).await?.json().await?)
    }

    fn table(&self, session: &Session, req: RequestBuilder) -> RequestBuilder {
        req.header(AUTHORIZATION, format!("Bearer {}", session.access_token))
            .header(CONTENT_TYPE, "application/json")
    }
}

impl From<TokenResponse> for Session {
    fn from(token: TokenResponse) -> Self {
        Session {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_at,
            user_id: token.user.id,
            email: token.user.email,
        }
    }
}

/// Turns a non-2xx response into a readable error using whatever message
/// shape GoTrue or PostgREST returned.
async fn check(res: Response) -> Result<Response> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    let body = res.text().await.unwrap_or_default();
    let message = serde_json::from_str::<ApiError>(&body)
        .ok()
        .and_then(|e| {
            let mut parts = vec![e.message?];
            parts.extend(e.details);
            parts.extend(e.hint);
            Some(parts.join(" — "))
        })
        .unwrap_or_else(|| body.trim().to_string());
    if message.is_empty() {
        bail!("Supabase returned {status}");
    }
    bail!("Supabase returned {status}: {message}");
}
