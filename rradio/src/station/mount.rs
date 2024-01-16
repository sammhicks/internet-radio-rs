use std::path::Path;

use rradio_messages::{arcstr, MountError};

use sys_mount::Unmount;

pub struct Handle {
    _mount: sys_mount::UnmountDrop<sys_mount::Mount>,
    mounted_directory: tempfile::TempDir,
}

impl Handle {
    pub fn mounted_directory(&self) -> &Path {
        self.mounted_directory.path()
    }
}

trait MountBuilderExt<'a> {
    fn maybe_data<T: AsRef<str>>(self, data: &'a Option<T>) -> Self;
}

impl<'a> MountBuilderExt<'a> for sys_mount::MountBuilder<'a> {
    fn maybe_data<T: AsRef<str>>(self, data: &'a Option<T>) -> Self {
        if let Some(data) = data {
            self.data(data.as_ref())
        } else {
            self
        }
    }
}

pub(super) trait Mount {
    fn device(&self) -> &str;
    fn file_system_type(&self) -> &str;

    fn maybe_data(&self) -> Option<&str> {
        None
    }

    fn mount(&self) -> std::result::Result<Handle, MountError> {
        let mounted_directory = tempfile::Builder::new()
            .prefix("rradio")
            .tempdir()
            .map_err(|err| {
                MountError::CouldNotCreateTemporaryDirectory(arcstr::format!("{err}"))
            })?;

        let mount = sys_mount::Mount::builder()
            .fstype(self.file_system_type())
            .flags(sys_mount::MountFlags::RDONLY | sys_mount::MountFlags::NOATIME)
            .maybe_data(&self.maybe_data())
            .mount(self.device(), &mounted_directory)
            .map_err(|err| {
                if let std::io::ErrorKind::NotFound = err.kind() {
                    MountError::NotFound
                } else {
                    MountError::CouldNotMountDevice {
                        device: self.device().into(),
                        err: arcstr::format!("{err}"),
                    }
                }
            })?
            .into_unmount_drop(sys_mount::UnmountFlags::DETACH);

        Ok(Handle {
            _mount: mount,
            mounted_directory,
        })
    }
}
