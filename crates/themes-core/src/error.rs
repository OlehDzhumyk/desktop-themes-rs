// SPDX-License-Identifier: GPL-3.0-or-later
use std::fmt;

#[derive(Debug)]
pub struct Error(pub String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(format!("помилка вводу-виводу: {e}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error(format!("помилка JSON: {e}"))
    }
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error(format!("D-Bus: {e}"))
    }
}

#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::Error(format!($($t)*))) };
}

/// Додає контекст (шлях тощо) до помилки вводу-виводу.
pub trait Context<T> {
    fn ctx(self, what: impl fmt::Display) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn ctx(self, what: impl fmt::Display) -> Result<T> {
        self.map_err(|e| Error(format!("{what}: {e}")))
    }
}
