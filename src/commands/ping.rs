use std::{net::Ipv4Addr, time::Duration};

use lazy_regex::regex;
use pinger::{PingOptions, PingResult};
use teloxide::prelude::*;
use url::Url;

use crate::commands::bot_init::ChatRequest;
use crate::commands::errors::BotPingError;

static IP_RE: &lazy_regex::Lazy<lazy_regex::Regex> =
    regex!(r"^\d{1,3}[.]\d{1,3}[.]\d{1,3}[.]\d{1,3}$");

pub struct PingCmd<'cr, 'host> {
    pub chat_request: &'cr ChatRequest,
    pub host: &'host str,
}

impl PingCmd<'_, '_> {
    pub fn new<'a>(chat_request: &'a ChatRequest, host: &'a str) -> PingCmd<'a, 'a> {
        PingCmd { chat_request, host }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        let text = match self.ping() {
            Ok(report) => report,
            Err(err) => err.to_string(),
        };
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, text)
            .await
    }

    fn ping_formatter(&self, ping_result: PingResult) -> String {
        match ping_result {
            PingResult::Pong(duration, line) => format!("[ {:?} ] => {}", duration, line),
            PingResult::Timeout(_) => "Timeout!".to_string(),
            PingResult::Unknown(line) => format!("Unknown line: {}", line),
            PingResult::PingExited(code, stderr) => format!("code: {} err: {}", code, stderr),
        }
    }

    fn extract_host_from_url(user_input: &str) -> Result<String, BotPingError> {
        let parsed = Url::parse(user_input).map_err(|_| BotPingError::BadUrl)?;
        Ok(parsed
            .host_str()
            .unwrap_or("No target host found")
            .to_string())
    }

    fn extract_host_from_ip(user_input: &str) -> Result<String, BotPingError> {
        user_input
            .parse::<Ipv4Addr>()
            .map(|_| user_input.to_string())
            .map_err(|_| BotPingError::NotAHost)
    }

    fn normalize_host(&self, user_input: &str) -> Result<String, BotPingError> {
        let host = user_input.trim();
        if host.contains("://") {
            Self::extract_host_from_url(host)
        } else if IP_RE.is_match(host) {
            Self::extract_host_from_ip(host)
        } else {
            Ok(host.to_string())
        }
    }

    fn ping(&self) -> Result<String, BotPingError> {
        let target = self.normalize_host(self.host)?;
        let options = PingOptions::new(target, Duration::from_secs(1), None);
        let receiver = pinger::ping(options)?;
        let result = receiver.recv()?;
        Ok(self.ping_formatter(result))
    }
}
