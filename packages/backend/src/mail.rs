//! Sending email. SMTP settings come from the environment (`SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`,
//! `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_TLS` = `starttls` | `tls` | `none`) and from the database (the super
//! admin's settings page); a field set in the database overrides the environment. Without a host and a
//! from address email is simply off.

use std::sync::Arc;

use async_trait::async_trait;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config;
use crate::models::EmailSettings;

/// A plain-text email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub text: String,
}

/// Sends mail. The real one speaks SMTP; tests swap in one that records.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Whether mail can be sent at all (settings are complete).
    async fn configured(&self) -> bool;
    async fn send(&self, mail: Mail) -> Result<(), String>;
}

/// The effective SMTP settings, with where each came from for the settings page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: Option<String>,
    /// `starttls`, `tls` or `none`.
    pub tls: String,
}

impl SmtpConfig {
    pub fn complete(&self) -> bool {
        self.host.as_deref().is_some_and(|h| !h.is_empty())
            && self.from.as_deref().is_some_and(|f| !f.is_empty())
    }
}

/// The settings from the environment alone.
pub fn env_config() -> SmtpConfig {
    SmtpConfig {
        host: config::var("SMTP_HOST"),
        port: config::var("SMTP_PORT").and_then(|p| p.parse().ok()),
        username: config::var("SMTP_USER"),
        password: config::var("SMTP_PASSWORD"),
        from: config::var("SMTP_FROM"),
        tls: config::var("SMTP_TLS").unwrap_or_else(|| "starttls".to_string()),
    }
}

/// The settings in force: each field set in the database wins over the environment.
pub async fn effective_config(db: &mut dyn toasty::Executor) -> SmtpConfig {
    let mut config = env_config();
    if let Ok(Some(row)) = EmailSettings::filter_by_id(1).first().exec(db).await {
        if let Some(host) = config::non_empty(row.host) {
            config.host = Some(host);
        }
        if let Some(port) = row.port.and_then(|p| u16::try_from(p).ok()) {
            config.port = Some(port);
        }
        if let Some(user) = config::non_empty(row.username) {
            config.username = Some(user);
        }
        if let Some(password) = config::non_empty(row.password) {
            config.password = Some(password);
        }
        if let Some(from) = config::non_empty(row.sender) {
            config.from = Some(from);
        }
        if let Some(tls) = config::non_empty(row.tls) {
            config.tls = tls;
        }
    }
    config
}

/// Build the SMTP transport for a complete config.
fn transport(config: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let host = config.host.as_deref().ok_or("no SMTP host")?;
    let builder = match config.tls.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        "none" => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            host,
        )),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
    }
    .map_err(|e| e.to_string())?;
    let builder = match config.port {
        Some(port) => builder.port(port),
        None => builder,
    };
    let builder = match (&config.username, &config.password) {
        (Some(user), Some(password)) => {
            builder.credentials(Credentials::new(user.clone(), password.clone()))
        }
        _ => builder,
    };
    Ok(builder.build())
}

/// Sends through SMTP, reading the settings from the database and environment each time (so a change on
/// the settings page applies at once).
pub struct SmtpMailer {
    db: toasty::Db,
}

impl SmtpMailer {
    pub fn shared(db: toasty::Db) -> Arc<dyn Mailer> {
        Arc::new(Self { db })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn configured(&self) -> bool {
        effective_config(&mut self.db.clone()).await.complete()
    }

    async fn send(&self, mail: Mail) -> Result<(), String> {
        let config = effective_config(&mut self.db.clone()).await;
        if !config.complete() {
            return Err("email is not configured".to_string());
        }
        let from: Mailbox = config
            .from
            .as_deref()
            .unwrap_or_default()
            .parse()
            .map_err(|e| format!("the from address is invalid: {e}"))?;
        let to: Mailbox = mail
            .to
            .parse()
            .map_err(|e| format!("the recipient is invalid: {e}"))?;
        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(mail.subject)
            .body(mail.text)
            .map_err(|e| e.to_string())?;
        transport(&config)?
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// A mailer that records what it is asked to send (tests).
#[derive(Default)]
pub struct Outbox {
    pub sent: std::sync::Mutex<Vec<Mail>>,
}

#[async_trait]
impl Mailer for Outbox {
    async fn configured(&self) -> bool {
        true
    }

    async fn send(&self, mail: Mail) -> Result<(), String> {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(mail);
        Ok(())
    }
}

/// Email an invite link (when mail is configured and `PUBLIC_URL` says where the app lives), in the
/// background. Best effort: a failure is only logged, the admin still sees the link.
pub fn send_invite(mailer: Arc<dyn Mailer>, to: String, name: String, token: String) {
    let Some(base) = config::public_url() else {
        return;
    };
    tokio::spawn(async move {
        if !mailer.configured().await {
            return;
        }
        let greeting = if name.trim().is_empty() {
            "Hello".to_string()
        } else {
            format!("Hello {name}")
        };
        let mail = Mail {
            to,
            subject: "You have been invited to Woodmouse".to_string(),
            text: format!(
                "{greeting},\n\nYou have been invited to Woodmouse. Choose your password here (the link works once and expires in 7 days):\n\n{}/invite/{token}\n",
                base.trim_end_matches('/')
            ),
        };
        if let Err(e) = mailer.send(mail).await {
            tracing::warn!(error = %e, "sending an invite email failed");
        }
    });
}
