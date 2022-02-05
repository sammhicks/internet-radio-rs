use sys_mount::Unmount;

use super::super::Credentials;
use super::Handle;

use rradio_messages::MountError;

type Result<T> = std::result::Result<T, rradio_messages::MountError>;

pub(super) fn mount(
    device: &str,
    file_system_type: &str,
    credentials: Option<&Credentials>,
) -> Result<Handle> {
    let mounted_directory = tempdir::TempDir::new("rradio")
        .map_err(|err| MountError::CouldNotCreateTemporaryDirectory(err.to_string().into()))?;

    let data = credentials.map(|credentials| {
        format!(
            "user={},pass={},vers=3.0",
            credentials.username, credentials.password
        )
    });

    let mount = sys_mount::Mount::new(
        device,
        &mounted_directory,
        file_system_type,
        sys_mount::MountFlags::RDONLY | sys_mount::MountFlags::NOATIME,
        data.as_deref(),
    )
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
