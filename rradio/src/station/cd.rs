#![allow(clippy::upper_case_acronyms)]

use std::fmt::Debug;

use super::Track;

use rradio_messages::{arcstr, CdError, EjectError};

type Result<T> = std::result::Result<T, rradio_messages::CdError>;

trait Parameter {
    fn into_raw(self) -> libc::c_ulong;
}

#[derive(Copy, Clone, Debug)]
struct NoParameter;

impl Parameter for NoParameter {
    fn into_raw(self) -> libc::c_ulong {
        0
    }
}

#[repr(C)]
#[derive(Debug, Default)]
struct CdToc {
    cdth_trk0: u8, /* start track */
    cdth_trk1: u8, /* end track */
}

impl Parameter for *mut CdToc {
    fn into_raw(self) -> libc::c_ulong {
        self as libc::c_ulong
    }
}

#[repr(u8)]
#[derive(Debug)]
enum LbaMsf {
    Lba = 0x01,
    Msf = 0x02,
}

impl Default for LbaMsf {
    fn default() -> Self {
        Self::Lba
    }
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct cdrom_msf0 {
    minute: u8,
    second: u8,
    frame: u8,
}

#[repr(C)]
union cdrom_addr {
    msf: cdrom_msf0,
    lba: libc::c_int,
}

impl Default for cdrom_addr {
    fn default() -> Self {
        Self { lba: 0 }
    }
}

#[repr(C)]
#[derive(Default)]
struct AdrCtrl(u8);
impl AdrCtrl {
    fn adr(&self) -> u8 {
        #[cfg(target_endian = "big")]
        {
            self.0 >> 4
        }
        #[cfg(target_endian = "little")]
        {
            self.0 & 0b1111
        }
    }

    fn ctrl(&self) -> u8 {
        #[cfg(target_endian = "big")]
        {
            self.0 & 0b1111
        }
        #[cfg(target_endian = "little")]
        {
            self.0 >> 4
        }
    }

    fn debug_fields<'a, 'b: 'a>(&self, s: &mut std::fmt::DebugStruct<'a, 'b>) {
        let adr = self.adr();
        let ctrl = self.ctrl();
        s.field("cdte_adr", &adr).field("cdte_ctrl", &ctrl);
    }
}

impl Debug for AdrCtrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("AdrCtrl");
        self.debug_fields(&mut debug_struct);
        debug_struct.finish()
    }
}

#[repr(C)]
#[derive(Default)]
struct CdTocEntry {
    cdte_track: u8,
    cdte_adr_ctrl: AdrCtrl,
    cdte_format: LbaMsf,
    cdte_addr: cdrom_addr,
    cdte_datamode: u8,
}

impl Debug for CdTocEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("CdTocEntry");
        debug_struct.field("cdte_track", &self.cdte_track);
        self.cdte_adr_ctrl.debug_fields(&mut debug_struct);
        debug_struct.field("cdte_format", &self.cdte_format);
        match self.cdte_format {
            LbaMsf::Lba => debug_struct.field("cdte_addr", unsafe { &self.cdte_addr.lba }),
            LbaMsf::Msf => debug_struct.field("cdte_addr", unsafe { &self.cdte_addr.msf }),
        };
        debug_struct.field("cdte_datamode", &self.cdte_datamode);
        debug_struct.finish()
    }
}

impl Parameter for *mut CdTocEntry {
    fn into_raw(self) -> libc::c_ulong {
        self as libc::c_ulong
    }
}

#[repr(u8)]
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
enum LockDoor {
    Unlock = 0,
    Lock = 1,
}

impl Parameter for LockDoor {
    fn into_raw(self) -> libc::c_ulong {
        self as libc::c_ulong
    }
}

trait Request {
    type Parameter: Parameter;
    fn into_raw(self) -> libc::c_ulong;
}

macro_rules! generate_requests {
    ($(#define $name:ident $code:literal)*) => {
        $(
            #[allow(non_camel_case_types, dead_code)]
            #[derive(Clone, Copy, Debug)]
            struct $name;

            impl $name {
                #[allow(dead_code)]
                const CODE: libc::c_ulong = $code;
            }
        )*
    };
}

generate_requests!(
#define CDROMREADTOCHDR         0x5305 /* Read TOC header
                                           (struct cdrom_tochdr) */
#define CDROMREADTOCENTRY       0x5306 /* Read TOC entry
                                           (struct cdrom_tocentry) */
#define CDROMSTOP               0x5307 /* Stop the cdrom drive */
#define CDROMEJECT              0x5309 /* Ejects the cdrom media */

#define CDROM_DRIVE_STATUS      0x5326  /* Get tray position, etc. */
#define CDROM_DISC_STATUS       0x5327  /* Get disc type, etc. */
#define CDROM_LOCKDOOR          0x5329  /* lock or unlock door */
);

macro_rules! requests_with_no_parameter {
    ($($name:ident),*) => {
        $(
            impl Request for $name {
                type Parameter = NoParameter;
                fn into_raw(self) -> libc::c_ulong {
                    Self::CODE
                }
            }
        )*
    };
}

requests_with_no_parameter!(CDROMSTOP, CDROMEJECT, CDROM_DRIVE_STATUS, CDROM_DISC_STATUS);

macro_rules! request_parameter {
    ($name:ident, $parameter:ty) => {
        impl Request for $name {
            type Parameter = $parameter;
            fn into_raw(self) -> libc::c_ulong {
                Self::CODE
            }
        }
    };
}

request_parameter!(CDROMREADTOCHDR, *mut CdToc);
request_parameter!(CDROMREADTOCENTRY, *mut CdTocEntry);
request_parameter!(CDROM_LOCKDOOR, LockDoor);

trait FileExt: std::os::unix::io::AsRawFd {
    fn ioctl<R: Request<Parameter = NoParameter>>(
        &mut self,
        request: R,
    ) -> std::io::Result<libc::c_int> {
        self.ioctl_with_parameter(request, NoParameter)
    }

    fn ioctl_with_parameter<R: Request>(
        &mut self,
        request: R,
        parameter: R::Parameter,
    ) -> std::io::Result<libc::c_int> {
        let result =
            unsafe { libc::ioctl(self.as_raw_fd(), request.into_raw(), parameter.into_raw()) };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }
}

impl FileExt for std::fs::File {}

async fn poll_ioctl<F, R>(f: &mut F, request: R) -> std::io::Result<libc::c_int>
where
    F: FileExt,
    R: Request<Parameter = NoParameter> + Copy + Debug,
{
    poll_ioctl_with_parameter(f, request, NoParameter).await
}

async fn poll_ioctl_with_parameter<F, R>(
    f: &mut F,
    request: R,
    parameter: R::Parameter,
) -> std::io::Result<libc::c_int>
where
    F: FileExt,
    R: Request + Copy + Debug,
    R::Parameter: Copy + Debug,
{
    use std::time::{Duration, Instant};
    let start_time = Instant::now();
    loop {
        let error = match f.ioctl_with_parameter(request, parameter) {
            Ok(code) => break Ok(code),
            Err(err) => err,
        };

        if start_time.elapsed() > Duration::from_secs(3) {
            tracing::error!("{:?} ({:?}): {}", request, parameter, error);
            return Err(error);
        }

        tracing::warn!("{:?} ({:?}): {}", request, parameter, error);

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[allow(clippy::needless_pass_by_value)]
fn ioctl_error(err: std::io::Error) -> CdError {
    CdError::IoCtlError {
        code: err.raw_os_error(),
        message: arcstr::format!("{err}"),
    }
}

pub fn tracks(device: &str) -> Result<Vec<Track>> {
    let mut device = std::fs::File::open(device).map_err(|err| CdError::FailedToOpenDevice {
        code: err.raw_os_error(),
        message: arcstr::format!("{err}"),
    })?;

    match device.ioctl(CDROM_DRIVE_STATUS).map_err(ioctl_error)? {
        0 => return Err(CdError::NoCdInfo),         // CDS_NO_INFO
        1 => return Err(CdError::NoCd),             // CDS_NO_DISC
        2 => return Err(CdError::CdTrayIsOpen),     // CDS_TRAY_OPEN
        3 => return Err(CdError::CdTrayIsNotReady), // CDS_DRIVE_NOT_READY
        4 => tracing::debug!("CD drive OK"),        // CDS_DISC_OK
        n => return Err(CdError::UnknownDriveStatus(n as isize)),
    }

    match device.ioctl(CDROM_DISC_STATUS).map_err(ioctl_error)? {
        0 => return Err(CdError::NoCdInfo),         // CDS_NO_INFO
        1 => return Err(CdError::NoCd),             // CDS_NO_DISC
        2 => return Err(CdError::CdTrayIsOpen),     // CDS_TRAY_OPEN
        3 => return Err(CdError::CdTrayIsNotReady), // CDS_DRIVE_NOT_READY
        100 => tracing::debug!("Audio CD"),         // CDS_AUDIO
        101 => return Err(CdError::CdIsData1),      // CDS_DATA_1
        102 => return Err(CdError::CdIsData2),      // CDS_DATA_2
        103 => return Err(CdError::CdIsXA21),       // CDS_XA_2_1
        104 => return Err(CdError::CdIsXA22),       // CDS_XA_2_2
        105 => tracing::debug!("Mixed CD"),         // CDS_MIXED
        n => return Err(CdError::UnknownDriveStatus(n as isize)),
    }

    let mut toc = CdToc::default();

    device
        .ioctl_with_parameter(CDROMREADTOCHDR, &mut toc)
        .map_err(ioctl_error)?;

    tracing::debug!("CD toc: {:?}", toc);

    (toc.cdth_trk0..=toc.cdth_trk1)
        .filter_map(|track_index| cd_track(&mut device, track_index, toc.cdth_trk1).transpose())
        .collect()
}

fn cd_track(device: &mut std::fs::File, track_index: u8, track_count: u8) -> Result<Option<Track>> {
    let mut toc_entry = CdTocEntry {
        cdte_track: track_index,
        cdte_format: LbaMsf::Msf,
        ..CdTocEntry::default()
    };

    device
        .ioctl_with_parameter(CDROMREADTOCENTRY, &mut toc_entry)
        .map_err(ioctl_error)?;

    tracing::debug!("{:?}", toc_entry);

    if (0b0100 & toc_entry.cdte_adr_ctrl.ctrl()) > 0 {
        // This is a data track
        Ok(None)
    } else {
        Ok(Some(Track {
            title: Some(rradio_messages::arcstr::format!(
                "Track {} of {}",
                track_index,
                track_count
            )),
            album: None,
            artist: None,
            url: rradio_messages::arcstr::format!("cdda://{}", track_index),
            is_notification: false,
        }))
    }
}

pub async fn eject<P: AsRef<std::path::Path> + Copy>(
    path: P,
) -> std::result::Result<(), EjectError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut device = std::fs::OpenOptions::new()
        .custom_flags(libc::O_NONBLOCK)
        .read(true)
        .open(path)
        .map_err(|_| EjectError::FailedToOpenDevice)?;

    poll_ioctl_with_parameter(&mut device, CDROM_LOCKDOOR, LockDoor::Unlock)
        .await
        .ok();

    poll_ioctl(&mut device, CDROMEJECT)
        .await
        .map_err(|_| EjectError::FailedToEjectDevice)?;

    Ok(())
}
