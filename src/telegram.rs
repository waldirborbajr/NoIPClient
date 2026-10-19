use anyhow::{anyhow, Result};

use crate::{config::TelegramConfig, http::agent};

pub(crate) fn notify_telegram(
    cfg: &Option<TelegramConfig>,
    text: &str,
    silent: bool,
) -> Result<()> {
    let Some(tg) = cfg else {
        return Ok(());
    };
    if !tg.enabled() {
        return Ok(());
    }
    let token = tg.token.as_deref().unwrap();
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");

    let mut last_err: Option<anyhow::Error> = None;
    for chat in &tg.chat_ids {
        let body = ureq::json!({
            "chat_id": chat,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
            "disable_notification": silent,
        });
        let res = agent()
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map(|_| ())
            .map_err(|e| anyhow!(e.to_string().replace(token, "***")));
        if let Err(e) = res {
            last_err = Some(e);
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
