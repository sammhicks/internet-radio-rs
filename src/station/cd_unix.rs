use anyhow::{Context, Result};

use super::Track;

#[repr(C)]
#[derive(Debug, Default)]
struct CdToc {
    cdth_trk0: u8, /* start track */
    cdth_trk1: u8, /* end track */
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

#[repr(C)]
#[derive(Copy, Clone, Debug)]
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

impl std::fmt::Debug for AdrCtrl {
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

impl std::fmt::Debug for CdTocEntry {
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

#[allow(non_camel_case_types)]
#[repr(u32)]
enum IoCtlRequest {
    CDROMREADTOCHDR = 0x5305,
    CDROMREADTOCENTRY = 0x5306,
    CDROM_DRIVE_STATUS = 0x5326,
    CDROM_DISC_STATUS = 0x5327,
}

unsafe fn check_errno(result: libc::c_int) -> Result<()> {
    if result < 0 {
        let message_cstr = std::ffi::CStr::from_ptr(libc::strerror(*libc::__errno_location()));
        let message = message_cstr.to_string_lossy().into_owned();
        anyhow::bail!(message);
    }
    Ok(())
}

pub fn tracks(device: &str) -> Result<Vec<Track>> {
    use libc::c_uint;
    use std::os::unix::io::AsRawFd;

    let device = std::fs::File::open(device).context("Cannot open CD device")?;

    let fd = device.as_raw_fd();

    match unsafe { libc::ioctl(fd, IoCtlRequest::CDROM_DRIVE_STATUS as c_uint, 0) } {
        0 => anyhow::bail!("No CD info"),           // CDS_NO_INFO
        1 => anyhow::bail!("No CD"),                // CDS_NO_DISC
        2 => anyhow::bail!("CD tray is open"),      // CDS_TRAY_OPEN
        3 => anyhow::bail!("CD tray is not ready"), // CDS_DRIVE_NOT_READY
        4 => log::debug!("CD drive OK"),            // CDS_DISC_OK
        n => anyhow::bail!("Unknown CDROM_DRIVE_STATUS: {}", n),
    }

    match unsafe { libc::ioctl(fd, IoCtlRequest::CDROM_DISC_STATUS as c_uint, 0) } {
        0 => anyhow::bail!("No CD info"),           // CDS_NO_INFO
        1 => anyhow::bail!("No CD"),                // CDS_NO_DISC
        2 => anyhow::bail!("CD tray is open"),      // CDS_TRAY_OPEN
        3 => anyhow::bail!("CD tray is not ready"), // CDS_DRIVE_NOT_READY
        100 => log::debug!("Audio CD"),             // CDS_AUDIO
        101 => anyhow::bail!("CD is CDS_DATA_1"),   // CDS_DATA_1
        102 => anyhow::bail!("CD is CDS_DATA_2"),   // CDS_DATA_2
        103 => anyhow::bail!("CD is CDS_XA_2_1"),   // CDS_XA_2_1
        104 => anyhow::bail!("CD is CDS_XA_2_2"),   // CDS_XA_2_2
        105 => log::debug!("Mixed CD"),             // CDS_MIXED
        n => anyhow::bail!("Unknown CDROM_DISC_STATUS: {}", n),
    }

    let mut toc = CdToc::default();

    unsafe {
        check_errno(libc::ioctl(
            fd,
            IoCtlRequest::CDROMREADTOCHDR as c_uint,
            (&mut toc) as *mut CdToc,
        ))?;
    }

    log::debug!("CD toc: {:?}", toc);

    (toc.cdth_trk0..=toc.cdth_trk1)
        .filter_map(|track_index| cd_track(fd, track_index).transpose())
        .collect()
}

fn cd_track(fd: libc::c_int, track_index: u8) -> Result<Option<Track>> {
    let mut toc_entry = CdTocEntry {
        cdte_track: track_index,
        cdte_format: LbaMsf::Msf,
        ..CdTocEntry::default()
    };

    unsafe {
        check_errno(libc::ioctl(
            fd,
            IoCtlRequest::CDROMREADTOCENTRY as libc::c_uint,
            (&mut toc_entry) as *mut CdTocEntry,
        ))?
    };

    log::debug!("{:?}", toc_entry);

    if (0b0100 & toc_entry.cdte_adr_ctrl.ctrl()) > 0 {
        // This is a data track
        Ok(None)
    } else {
        Ok(Some(Track {
            title: Some(format!("Track {}", track_index)),
            url: format!("cdda://{}", track_index),
            is_notification: false,
        }))
    }}
