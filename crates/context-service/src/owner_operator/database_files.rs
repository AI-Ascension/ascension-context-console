use super::protected_files::{AdapterError, PrivateRoot, validate_name};

#[cfg(unix)]
use super::protected_files::{FileSnapshot, verify_private_file};

impl PrivateRoot {
    pub(super) fn database_path(&self, name: &str) -> Result<std::path::PathBuf, AdapterError> {
        validate_name(name)?;
        self.verify_current_root()?;
        Ok(self.configured_path().join(name))
    }

    pub(super) fn ensure_database_file(&self, name: &str) -> Result<(u64, u64), AdapterError> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, fchmod, openat};
            use rustix::io::Errno;
            use std::os::fd::AsFd;

            validate_name(name)?;
            self.verify_current_root()?;
            let directory = self.directory_file()?;
            let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
            let fd = match openat(
                directory.as_fd(),
                name,
                flags | OFlags::CREAT | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    fchmod(&fd, Mode::RUSR | Mode::WUSR).map_err(|_| AdapterError::Unavailable)?;
                    fd
                }
                Err(error) if error == Errno::EXIST => {
                    openat(directory.as_fd(), name, flags, Mode::empty())
                        .map_err(|_| AdapterError::Unavailable)?
                }
                Err(_) => return Err(AdapterError::Unavailable),
            };
            let file = std::fs::File::from(fd);
            verify_private_file(&file)?;
            let snapshot = FileSnapshot::read(&file)?;
            self.verify_database_files(name, Some((snapshot.dev, snapshot.ino)))?;
            Ok((snapshot.dev, snapshot.ino))
        }
        #[cfg(not(unix))]
        {
            let _ = name;
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn verify_database_files(
        &self,
        name: &str,
        expected: Option<(u64, u64)>,
    ) -> Result<(u64, u64), AdapterError> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, openat};
            use rustix::io::Errno;
            use std::os::fd::AsFd;

            validate_name(name)?;
            self.verify_current_root()?;
            let directory = self.directory_file()?;
            let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
            let fd = openat(directory.as_fd(), name, flags, Mode::empty())
                .map_err(|_| AdapterError::Unavailable)?;
            let database = std::fs::File::from(fd);
            verify_private_file(&database)?;
            let snapshot = FileSnapshot::read(&database)?;
            if expected.is_some_and(|identity| identity != (snapshot.dev, snapshot.ino)) {
                return Err(AdapterError::Unavailable);
            }
            for suffix in ["-wal", "-shm", "-journal"] {
                let sidecar = format!("{name}{suffix}");
                match openat(directory.as_fd(), sidecar.as_str(), flags, Mode::empty()) {
                    Ok(fd) => verify_private_file(&std::fs::File::from(fd))?,
                    Err(error) if error == Errno::NOENT => {}
                    Err(_) => return Err(AdapterError::Unavailable),
                }
            }
            self.verify_current_root()?;
            Ok((snapshot.dev, snapshot.ino))
        }
        #[cfg(not(unix))]
        {
            let _ = (name, expected);
            Err(AdapterError::UnsupportedPlatform)
        }
    }
}
