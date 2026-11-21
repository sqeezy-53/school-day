//! Local, one-shot device sessions. Schedule messages are only available after
//! TLS authentication against the saved keys on both sides.

mod exchange;
mod profile;
mod transport;

use std::{fmt, io};

pub use exchange::{Conflict, Outcome, Review, SessionMode};
pub use profile::{Profile, ensure_writable, local_write, recover};
use transport::pair_client;
pub use transport::{Cancel, Identity, Invitation, Pairing, Peer};

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Deadline,
    Cancelled,
    InvalidMessage,
    UntrustedDevice,
    InvalidAddress,
    InvitationExpired,
    Rejected,
    Crypto,
    Merge(crate::lessons::sync::Error),
    AlreadyPaired,
    ExchangePending,
    ScheduleChanged,
    TooManyConflicts,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "Связь прервалась или устройство недоступно",
            Self::Deadline => "Время ожидания истекло",
            Self::Cancelled | Self::Rejected => "Обмен отменён",
            Self::InvalidMessage => "Получены несовместимые или повреждённые данные",
            Self::UntrustedDevice => "Устройство не прошло проверку ключа",
            Self::InvalidAddress => "Укажите адрес в локальной сети",
            Self::InvitationExpired => "Приглашение больше не действует",
            Self::Merge(reason) => return write!(f, "{reason}"),
            Self::AlreadyPaired => "Сначала отключите ранее сопряжённое устройство",
            Self::ExchangePending => "Завершите прерванный обмен с доверенным устройством",
            Self::TooManyConflicts => "Слишком много конфликтов для одного обмена",
            Self::ScheduleChanged => "Расписание изменилось. Откройте новый обмен",
            Self::Crypto => "Не удалось установить защищённое соединение",
        })
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            Self::Deadline
        } else if error.kind() == io::ErrorKind::Interrupted {
            Self::Cancelled
        } else {
            Self::Io(error)
        }
    }
}

impl From<crate::lessons::sync::Error> for Error {
    fn from(error: crate::lessons::sync::Error) -> Self {
        Self::Merge(error)
    }
}
