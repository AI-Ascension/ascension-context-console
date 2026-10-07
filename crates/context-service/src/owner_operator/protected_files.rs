use std::path::Path;
use zeroize::Zeroizing;

const MAX_PRIVATE_FILE_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdapterError {
    UnsupportedPlatform,
    Invalid,
    Unavailable,
    Denied,
    KeyUnavailable,
}

pub(super) fn read_private_file(
    path: &Path,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, AdapterError> {
    if limit == 0 || limit > MAX_PRIVATE_FILE_BYTES {
        return Err(AdapterError::Invalid);
    }
    #[cfg(unix)]
    {
        let file = open_absolute(path, true)?;
        read_open_file(file, limit)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, limit);
        Err(AdapterError::UnsupportedPlatform)
    }
}

#[derive(Clone)]
pub(super) struct PrivateRoot {
    #[cfg(unix)]
    directory: std::sync::Arc<std::fs::File>,
    path: std::path::PathBuf,
}

impl PrivateRoot {
    pub(super) fn open(path: &Path) -> Result<Self, AdapterError> {
        #[cfg(unix)]
        {
            let file = open_absolute(path, false)?;
            Ok(Self {
                directory: std::sync::Arc::new(file),
                path: path.to_path_buf(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn read_file(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, AdapterError> {
        validate_name(name)?;
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, openat};
            use std::os::fd::AsFd;
            self.verify_path_identity()?;
            let fd = openat(
                self.directory.as_fd(),
                name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(|_| AdapterError::Unavailable)?;
            let bytes = read_open_file(std::fs::File::from(fd), limit)?;
            self.verify_path_identity()?;
            Ok(bytes)
        }
        #[cfg(not(unix))]
        {
            let _ = limit;
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn verify_current_root(&self) -> Result<(), AdapterError> {
        #[cfg(unix)]
        {
            self.verify_path_identity()
        }
        #[cfg(not(unix))]
        {
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn directory_file(&self) -> Result<&std::fs::File, AdapterError> {
        #[cfg(unix)]
        {
            Ok(self.directory.as_ref())
        }
        #[cfg(not(unix))]
        {
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn configured_path(&self) -> &Path {
        &self.path
    }

    pub(super) fn open_operator_lock_file(&self) -> Result<std::fs::File, AdapterError> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, fchmod, openat};
            use rustix::io::Errno;
            use std::os::fd::AsFd;

            const LOCK_NAME: &str = "owner-service.lock";
            self.verify_path_identity()?;
            let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
            let fd = match openat(
                self.directory.as_fd(),
                LOCK_NAME,
                flags | OFlags::CREAT | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    fchmod(&fd, Mode::RUSR | Mode::WUSR).map_err(|_| AdapterError::Unavailable)?;
                    fd
                }
                Err(error) if error == Errno::EXIST => {
                    openat(self.directory.as_fd(), LOCK_NAME, flags, Mode::empty())
                        .map_err(|_| AdapterError::Unavailable)?
                }
                Err(_) => return Err(AdapterError::Unavailable),
            };
            let file = std::fs::File::from(fd);
            verify_private_file(&file)?;
            let named = openat(
                self.directory.as_fd(),
                LOCK_NAME,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(|_| AdapterError::Unavailable)?;
            let named = std::fs::File::from(named);
            verify_private_file(&named)?;
            let opened = FileSnapshot::read(&file)?;
            let current = FileSnapshot::read(&named)?;
            if opened != current || opened.len != 0 {
                return Err(AdapterError::Unavailable);
            }
            self.verify_path_identity()?;
            Ok(file)
        }
        #[cfg(not(unix))]
        {
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    pub(super) fn open_existing_operator_lock_file(&self) -> Result<std::fs::File, AdapterError> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, openat};
            use std::os::fd::AsFd;

            const LOCK_NAME: &str = "owner-service.lock";
            self.verify_path_identity()?;
            let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
            let file = std::fs::File::from(
                openat(self.directory.as_fd(), LOCK_NAME, flags, Mode::empty())
                    .map_err(|_| AdapterError::Unavailable)?,
            );
            verify_private_file(&file)?;
            let named = std::fs::File::from(
                openat(self.directory.as_fd(), LOCK_NAME, flags, Mode::empty())
                    .map_err(|_| AdapterError::Unavailable)?,
            );
            verify_private_file(&named)?;
            let opened = FileSnapshot::read(&file)?;
            let current = FileSnapshot::read(&named)?;
            if opened != current || opened.len != 0 {
                return Err(AdapterError::Unavailable);
            }
            self.verify_path_identity()?;
            Ok(file)
        }
        #[cfg(not(unix))]
        {
            Err(AdapterError::UnsupportedPlatform)
        }
    }

    #[cfg(unix)]
    fn verify_path_identity(&self) -> Result<(), AdapterError> {
        use std::os::unix::fs::MetadataExt;
        let current = open_absolute(&self.path, false)?;
        verify_private_directory(&current)?;
        let retained = self
            .directory
            .metadata()
            .map_err(|_| AdapterError::Unavailable)?;
        let reopened = current.metadata().map_err(|_| AdapterError::Unavailable)?;
        if retained.dev() != reopened.dev()
            || retained.ino() != reopened.ino()
            || retained.uid() != reopened.uid()
        {
            return Err(AdapterError::Unavailable);
        }
        Ok(())
    }
}

pub(super) fn validate_name(value: &str) -> Result<(), AdapterError> {
    if value.is_empty()
        || value.len() > 96
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        Err(AdapterError::Invalid)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn open_absolute(path: &Path, regular: bool) -> Result<std::fs::File, AdapterError> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::os::fd::AsFd;
    use std::os::unix::ffi::OsStrExt;
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > 4096 {
        return Err(AdapterError::Invalid);
    }
    let mut current = std::fs::File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| AdapterError::Unavailable)?,
    );
    verify_trusted_directory(&current, false)?;
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(AdapterError::Invalid);
    }
    for (index, component) in components.iter().enumerate().skip(1) {
        let std::path::Component::Normal(name) = component else {
            return Err(AdapterError::Invalid);
        };
        let last = index + 1 == components.len();
        let flags = if last && regular {
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK
        } else {
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW
        };
        let next = std::fs::File::from(
            openat(current.as_fd(), name, flags, Mode::empty())
                .map_err(|_| AdapterError::Unavailable)?,
        );
        if last {
            if regular {
                verify_trusted_directory(&current, true)?;
                verify_private_file(&next)?;
            } else {
                verify_trusted_directory(&current, false)?;
                verify_private_directory(&next)?;
            }
            return Ok(next);
        }
        verify_trusted_directory(&next, false)?;
        current = next;
    }
    Err(AdapterError::Invalid)
}

#[cfg(unix)]
pub(super) fn read_open_file(
    mut file: std::fs::File,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, AdapterError> {
    use std::io::Read;
    if limit == 0 || limit > MAX_PRIVATE_FILE_BYTES {
        return Err(AdapterError::Invalid);
    }
    verify_private_file(&file)?;
    let before = FileSnapshot::read(&file)?;
    let expected = usize::try_from(before.len).map_err(|_| AdapterError::Invalid)?;
    if expected > limit {
        return Err(AdapterError::Invalid);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(expected));
    (&mut file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| AdapterError::Unavailable)?;
    verify_private_file(&file)?;
    let after = FileSnapshot::read(&file)?;
    if before != after || bytes.len() != expected || bytes.len() > limit {
        return Err(AdapterError::Invalid);
    }
    Ok(bytes)
}

#[cfg(unix)]
#[derive(Eq, PartialEq)]
pub(super) struct FileSnapshot {
    pub(super) dev: u64,
    pub(super) ino: u64,
    pub(super) len: u64,
    uid: u32,
    links: u64,
    mode: u32,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[cfg(unix)]
impl FileSnapshot {
    pub(super) fn read(file: &std::fs::File) -> Result<Self, AdapterError> {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata().map_err(|_| AdapterError::Unavailable)?;
        Ok(Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            len: metadata.len(),
            uid: metadata.uid(),
            links: metadata.nlink(),
            mode: metadata.mode() & 0o7777,
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        })
    }
}

#[cfg(unix)]
fn verify_trusted_directory(
    file: &std::fs::File,
    require_private_service_owner: bool,
) -> Result<(), AdapterError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = file.metadata().map_err(|_| AdapterError::Unavailable)?;
    let owner = metadata.uid();
    let mode = metadata.permissions().mode() & 0o7777;
    let effective_uid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir()
        || (owner != 0 && owner != effective_uid)
        || mode & 0o0022 != 0
        || (require_private_service_owner && (owner != effective_uid || mode != 0o700))
    {
        return Err(AdapterError::Invalid);
    }
    Ok(())
}

#[cfg(unix)]
fn verify_private_directory(file: &std::fs::File) -> Result<(), AdapterError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = file.metadata().map_err(|_| AdapterError::Unavailable)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(AdapterError::Invalid);
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn verify_private_file(file: &std::fs::File) -> Result<(), AdapterError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = file.metadata().map_err(|_| AdapterError::Unavailable)?;
    verify_file_attributes(
        metadata.is_file(),
        metadata.uid(),
        metadata.nlink(),
        metadata.permissions().mode(),
        rustix::process::geteuid().as_raw(),
    )
}

#[cfg(unix)]
pub(super) fn verify_file_attributes(
    regular: bool,
    uid: u32,
    links: u64,
    mode: u32,
    expected_uid: u32,
) -> Result<(), AdapterError> {
    if regular && uid == expected_uid && links == 1 && mode & 0o7777 == 0o600 {
        Ok(())
    } else {
        Err(AdapterError::Invalid)
    }
}
