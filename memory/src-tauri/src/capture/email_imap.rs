//! IMAP poll source (Phase 5a).
//!
//! Config JSON shape:
//!
//! ```json
//! {
//!     "host": "imap.gmail.com",
//!     "port": 993,
//!     "username": "you@gmail.com",
//!     "folder": "Cairn"
//! }
//! ```
//!
//! Password lives in the OS keychain at account = `secret_account` field of
//! the row. The poller fetches **unseen messages** from the configured folder,
//! parses subject + body (text/plain preferred, falls back to text/html → plain),
//! and inserts them as notes with source = `email:<message-id>`.
//!
//! Dedupe key is the message UID; we never delete the message on the server,
//! we just refuse to re-import a UID we've already touched.

use crate::capture::{load_password, CaptureSource, PollOutcome};
use crate::db::Db;
use anyhow::{anyhow, Context, Result};
use async_imap::types::Fetch;
use mail_parser::MessageParser;
use rustls::{ClientConfig, RootCertStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_rustls::TlsConnector;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default = "default_folder")]
    pub folder: String,
}

fn default_folder() -> String {
    "Cairn".to_string()
}

pub async fn poll_once(db: &Db, source: &CaptureSource) -> Result<PollOutcome> {
    let cfg: ImapConfig = serde_json::from_str(&source.config).context("parse IMAP config JSON")?;
    let account = source
        .secret_account
        .as_deref()
        .ok_or_else(|| anyhow!("IMAP source has no secret_account"))?;
    let password = load_password(account)
        .await
        .context("read IMAP password from keychain")?;

    let mut session = connect_and_login(&cfg, &password).await?;
    session
        .select(&cfg.folder)
        .await
        .with_context(|| format!("SELECT folder `{}`", cfg.folder))?;

    // SEARCH UNSEEN — gives us UIDs we haven't read before from this client.
    let unseen = session
        .uid_search("UNSEEN")
        .await
        .context("SEARCH UNSEEN")?;
    let mut captured: u32 = 0;
    let mut skipped: u32 = 0;

    if unseen.is_empty() {
        let _ = session.logout().await;
        return Ok(PollOutcome {
            captured,
            seen_skipped: 0,
        });
    }

    let uid_list: Vec<String> = unseen.iter().map(|u| u.to_string()).collect();
    let uid_set = uid_list.join(",");

    use futures::StreamExt;
    let mut stream = session
        .uid_fetch(&uid_set, "(UID INTERNALDATE BODY[])")
        .await
        .context("FETCH BODY[]")?;

    while let Some(item) = stream.next().await {
        let f = item.context("imap fetch")?;
        match ingest(db, source, &f).await {
            Ok(true) => captured += 1,
            Ok(false) => skipped += 1,
            Err(e) => tracing::warn!(?e, "IMAP message ingest failed; skipping"),
        }
    }
    drop(stream);

    let _ = session.logout().await;
    Ok(PollOutcome {
        captured,
        seen_skipped: skipped,
    })
}

async fn connect_and_login(
    cfg: &ImapConfig,
    password: &str,
) -> Result<async_imap::Session<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>> {
    let tls = build_tls_config()?;
    let connector = TlsConnector::from(Arc::new(tls));

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let tcp = tokio::net::TcpStream::connect(&addr)
        .await
        .with_context(|| format!("connect {addr}"))?;
    let server_name: rustls_pki_types::ServerName<'_> = cfg
        .host
        .clone()
        .try_into()
        .map_err(|e| anyhow!("bad host name `{}`: {e}", cfg.host))?;
    let tls_stream = connector
        .connect(server_name, tcp)
        .await
        .context("TLS handshake")?;

    let client = async_imap::Client::new(tls_stream);
    let session = client
        .login(&cfg.username, password)
        .await
        .map_err(|(e, _client)| anyhow!("IMAP LOGIN failed: {e}"))?;
    Ok(session)
}

fn build_tls_config() -> Result<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Returns Ok(true) if a new note was created, Ok(false) if we'd seen this UID before.
async fn ingest(db: &Db, source: &CaptureSource, fetch: &Fetch) -> Result<bool> {
    let uid = fetch
        .uid
        .ok_or_else(|| anyhow!("IMAP fetch missing UID"))?
        .to_string();

    if db.has_seen(&source.id, &uid).await? {
        return Ok(false);
    }

    let raw = fetch
        .body()
        .ok_or_else(|| anyhow!("IMAP fetch missing body"))?;
    let msg = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow!("parse rfc822 message"))?;

    let subject = msg.subject().unwrap_or("(no subject)").trim().to_string();
    let from = msg
        .from()
        .and_then(|a| a.first())
        .and_then(|a| a.address())
        .unwrap_or("");
    let date = msg.date().map(|d| d.to_rfc3339()).unwrap_or_default();
    let body_text = msg
        .body_text(0)
        .map(|c| c.into_owned())
        .or_else(|| msg.body_html(0).map(|c| html_to_plain(&c)))
        .unwrap_or_default();

    let note_text = format!(
        "[email] {subject}\nfrom: {from}\ndate: {date}\n\n{}",
        truncate(&body_text, 8000)
    );
    db.insert_note(&note_text, &format!("email:{uid}")).await?;
    db.mark_seen(&source.id, &uid).await?;
    Ok(true)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Snap to a char boundary — a raw `&s[..max]` panics when a
        // multi-byte char (CJK mail bodies) straddles the cut.
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut out = s[..end].to_string();
        out.push_str("\n…(truncated)");
        out
    }
}

/// Crude HTML → text: strip tags but insert a space at every closing
/// `>` so inline-stripped words don't fuse together. Collapses whitespace
/// at the end. We don't need fidelity here — the LLM re-extracts structure
/// from whatever we give it.
fn html_to_plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_plain_strips_tags() {
        let h = "<p>hello <b>world</b></p><br/>line2";
        assert_eq!(html_to_plain(h), "hello world line2");
    }

    #[test]
    fn truncate_inserts_marker() {
        let t = truncate("abcdefghij", 5);
        assert!(t.starts_with("abcde"));
        assert!(t.contains("truncated"));
    }
}
