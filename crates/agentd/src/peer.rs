use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PeerIdentity {
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

#[cfg(target_os = "linux")]
pub(crate) fn identity(stream: &UnixStream) -> Result<PeerIdentity> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(cred).cast(),
            std::ptr::addr_of_mut!(len),
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to read peer credentials");
    }
    Ok(PeerIdentity {
        uid: cred.uid,
        gid: cred.gid,
    })
}

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
pub(crate) fn identity(stream: &UnixStream) -> Result<PeerIdentity> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let result = unsafe {
        libc::getpeereid(
            stream.as_raw_fd(),
            std::ptr::addr_of_mut!(uid),
            std::ptr::addr_of_mut!(gid),
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to read peer credentials");
    }
    Ok(PeerIdentity { uid, gid })
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd"
)))]
pub(crate) fn identity(_stream: &UnixStream) -> Result<PeerIdentity> {
    Err(anyhow::anyhow!(
        "peer credentials are unsupported on this platform"
    ))
}
