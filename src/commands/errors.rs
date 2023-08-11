use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum BotPingError {
    PingErrors(pinger::PingError),
    AnyHowErrors(anyhow::Error),
    RecvErr(std::sync::mpsc::RecvError),
    SimpleTextException(String),
    BadUrl,
    NotAHost
}

impl fmt::Display for BotPingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            BotPingError::SimpleTextException(ref err) => {
                write!(f, "{}", err)
            }
            BotPingError::PingErrors(ref err) => {
                write!(f, "{}", err)
            }
            BotPingError::AnyHowErrors(ref err) => {
                write!(f, "{}", err)
            }
            BotPingError::RecvErr(ref err) => {
                write!(f, "{}", err)
            }
            BotPingError::BadUrl => write!(f, "Bad URL"),
            BotPingError::NotAHost => write!(f, "Not a host"),
        }
    }
}

impl Error for BotPingError {}

impl From<String> for BotPingError {
    fn from(err: String) -> BotPingError {
        BotPingError::SimpleTextException(err)
    }
}

impl From<pinger::PingError> for BotPingError {
    fn from(err: pinger::PingError) -> BotPingError {
        BotPingError::PingErrors(err)
    }
}

impl From<anyhow::Error> for BotPingError {
    fn from(err: anyhow::Error) -> BotPingError {
        BotPingError::AnyHowErrors(err)
    }
}

impl From<std::sync::mpsc::RecvError> for BotPingError {
    fn from(err: std::sync::mpsc::RecvError) -> BotPingError {
        BotPingError::RecvErr(err)
    }
}
