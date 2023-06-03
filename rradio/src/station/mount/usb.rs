use sys_mount::Unmount;

use super::super::Credentials;
use super::Handle;

use rradio_messages::MountError;

type Result<T> = std::result::Result<T, rradio_messages::MountError>;

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

pub(super) fn mount(
    device: &str,
    file_system_type: &str,
    credentials: Option<&Credentials>,
) -> Result<Handle> {
    let mounted_directory = tempfile::Builder::new()
        .prefix("rradio")
        .tempdir()
        .map_err(|err| MountError::CouldNotCreateTemporaryDirectory(err.to_string().into()))?;

    let mount = sys_mount::Mount::builder()
        .fstype(file_system_type)
        .flags(sys_mount::MountFlags::RDONLY | sys_mount::MountFlags::NOATIME)
        .maybe_data(&credentials.map(|credentials| {
            format!(
                "user={},pass={},vers=3.0",
                credentials.username, credentials.password
            )
        }))
        .mount(device, &mounted_directory)
        .map_err(|err| {
            if let std::io::ErrorKind::NotFound = err.kind() {
                MountError::NotFound
            } else {
                MountError::CouldNotMountDevice {
                    device: device.into(),
                    err: err.to_string().into(),
                }
            }
        })?
        .into_unmount_drop(sys_mount::UnmountFlags::DETACH);

    Ok(Handle {
        _mount: mount,
        mounted_directory,
    })
}
