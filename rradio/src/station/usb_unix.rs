use sys_mount::Unmount;

use super::UsbHandle;

use rradio_messages::UsbError;

type Result<T> =
    std::result::Result<T, rradio_messages::UsbError<crate::atomic_string::AtomicString>>;

pub fn mount(device: &str) -> Result<UsbHandle> {
    let mounted_directory = tempdir::TempDir::new("rradio")
        .map_err(|err| UsbError::CouldNotCreateTemporaryDirectory(err.to_string().into()))?;

    let mount = sys_mount::Mount::new(
        device,
        &mounted_directory,
        "vfat",
        sys_mount::MountFlags::empty(),
        None,
    )
    .map_err(|err| {
        if let std::io::ErrorKind::NotFound = err.kind() {
            UsbError::UsbNotConnected
        } else {
            UsbError::CouldNotMountDevice {
                device: device.into(),
                err: err.to_string().into(),
            }
        }
    })?
    .into_unmount_drop(sys_mount::UnmountFlags::DETACH);

    Ok(UsbHandle {
        _mount: mount,
        mounted_directory,
    })
}
