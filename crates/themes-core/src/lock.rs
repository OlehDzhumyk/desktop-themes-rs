// SPDX-License-Identifier: GPL-3.0-or-later
use desk_settings::tr;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::paths::Ctx;

/// flock на `.lock` (той самий файл, що й у python-версії).
pub struct Lock(File);

impl Lock {
    pub fn acquire(ctx: &Ctx) -> Result<Lock> {
        std::fs::create_dir_all(&ctx.root)?;
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&ctx.lock)?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match f.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(Error(tr!(
                        "another process holds the lock {path}",
                        path = ctx.lock.display()
                    )));
                }
                Err(TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        f.set_len(0)?;
        let _ = write!(f, "{}", std::process::id());
        Ok(Lock(f))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
