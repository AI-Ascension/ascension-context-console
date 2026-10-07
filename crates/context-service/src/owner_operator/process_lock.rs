#[cfg(unix)]
use super::protected_files::FileSnapshot;
use super::protected_files::PrivateRoot;
use std::fs::File;

pub(super) struct OperatorProcessLock {
    _file: File,
    root: PrivateRoot,
    identity: (u64, u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LockError {
    Busy,
    Unavailable,
}

pub(super) fn acquire(root: &PrivateRoot) -> Result<OperatorProcessLock, LockError> {
    #[cfg(unix)]
    {
        use rustix::fs::{FlockOperation, flock};
        use rustix::io::Errno;
        root.verify_current_root()
            .map_err(|_| LockError::Unavailable)?;
        let file = root
            .open_operator_lock_file()
            .map_err(|_| LockError::Unavailable)?;
        let snapshot = FileSnapshot::read(&file).map_err(|_| LockError::Unavailable)?;
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(error) if error == Errno::WOULDBLOCK || error == Errno::AGAIN => {
                return Err(LockError::Busy);
            }
            Err(_) => return Err(LockError::Unavailable),
        }
        let lock = OperatorProcessLock {
            _file: file,
            root: root.clone(),
            identity: (snapshot.dev, snapshot.ino),
        };
        lock.check_current()?;
        Ok(lock)
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        Err(LockError::Unavailable)
    }
}

impl OperatorProcessLock {
    pub(super) fn check_current(&self) -> Result<(), LockError> {
        #[cfg(unix)]
        {
            self.root
                .verify_current_root()
                .map_err(|_| LockError::Unavailable)?;
            let current = self
                .root
                .open_existing_operator_lock_file()
                .map_err(|_| LockError::Unavailable)?;
            let snapshot = FileSnapshot::read(&current).map_err(|_| LockError::Unavailable)?;
            if (snapshot.dev, snapshot.ino) != self.identity {
                return Err(LockError::Unavailable);
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = self;
            Err(LockError::Unavailable)
        }
    }
}

pub(super) fn cli_error(error: LockError) -> &'static str {
    match error {
        LockError::Busy => "owner service is already active",
        LockError::Unavailable => "owner service lock is unavailable",
    }
}
