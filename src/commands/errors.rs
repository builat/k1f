use thiserror::Error;

/// Errors raised while preparing or running a ping.
#[derive(Debug, Error)]
pub enum BotPingError {
    #[error("Could not start ping: {0}")]
    Ping(#[from] pinger::PingCreationError),

    #[error("Ping receiver closed: {0}")]
    Recv(#[from] std::sync::mpsc::RecvError),

    #[error("Malformed URL")]
    BadUrl,

    #[error("Not a valid host")]
    NotAHost,
}
