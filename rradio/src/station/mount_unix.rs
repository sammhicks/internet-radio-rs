use sys_mount::Unmount;

use super::MountHandle;

use rradio_messages::MountError;

type Result<T> =
    std::result::Result<T, rradio_messages::MountError<crate::atomic_string::AtomicString>>;

pub fn mount(device: &str) -> Result<MountHandle> {
    let mounted_directory = tempdir::TempDir::new("rradio")
        .map_err(|err| MountError::CouldNotCreateTemporaryDirectory(err.to_string().into()))?;

    let mount = sys_mount::Mount::new(
        device,
        &mounted_directory,
        "vfat",
        sys_mount::MountFlags::RDONLY | sys_mount::MountFlags::NOATIME,
        None,
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

    Ok(MountHandle {
        _mount: mount,
        mounted_directory,
    })
}
