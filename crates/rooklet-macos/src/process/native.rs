//! macOS libproc adapter. No PID-only signaling fallback is allowed.
use super::engine::ProcessSystem;
use super::*;
use rooklet_core::process::TerminationMode;

pub(super) struct Native;
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::{Context, ensure};
    use std::{
        ffi::CStr,
        mem::{MaybeUninit, size_of},
    };
    // ABI declarations from Apple's xnu bsd/sys/proc_info_private.h:
    // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info_private.h
    // The unique identifier structure is explicitly declared a 56-byte API there.
    #[repr(C)]
    struct UniqueInfo {
        uuid: [u8; 16],
        unique_id: u64,
        parent_unique_id: u64,
        pid_version: i32,
        original_parent_version: i32,
        reserved: [u64; 2],
    }
    #[repr(C)]
    struct BsdUniqueInfo {
        bsd: libc::proc_bsdinfo,
        unique: UniqueInfo,
    }
    #[repr(C)]
    struct AuditToken {
        values: [u32; 8],
    }
    type AuditSignal = unsafe extern "C" fn(*mut AuditToken, libc::c_int) -> libc::c_int;
    const BSD_UNIQUE: i32 = 18;
    const SHORT_BSD: i32 = 13;
    fn bsd_unique_info(pid: u32) -> Result<BsdUniqueInfo> {
        let pid: i32 = pid.try_into().context("invalid process PID")?;
        let mut info = MaybeUninit::<BsdUniqueInfo>::zeroed();
        // SAFETY: flavor 18 writes proc_bsdinfowithuniqid, whose C layout is
        // BsdUniqueInfo above. The aligned buffer is writable for its full size
        // and remains alive for this synchronous call; libproc retains no pointer.
        let count = unsafe {
            libc::proc_pidinfo(
                pid,
                BSD_UNIQUE,
                0,
                info.as_mut_ptr().cast(),
                size_of::<BsdUniqueInfo>() as i32,
            )
        };
        ensure!(
            count == size_of::<BsdUniqueInfo>() as i32,
            "process identity is unavailable ({})",
            std::io::Error::last_os_error()
        );
        // SAFETY: the matching flavor returned the complete fixed-size buffer.
        // Every field is an integer or integer array, with no invalid bit patterns.
        Ok(unsafe { info.assume_init() })
    }
    fn short_bsd_info(pid: u32) -> Result<libc::proc_bsdshortinfo> {
        let pid: i32 = pid.try_into().context("invalid process PID")?;
        let mut info = MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
        // SAFETY: flavor 13 writes libc's SDK-matching proc_bsdshortinfo. The
        // aligned buffer is writable for its full size and alive for this
        // synchronous call; libproc retains no pointer.
        let count = unsafe {
            libc::proc_pidinfo(
                pid,
                SHORT_BSD,
                0,
                info.as_mut_ptr().cast(),
                size_of::<libc::proc_bsdshortinfo>() as i32,
            )
        };
        ensure!(
            count == size_of::<libc::proc_bsdshortinfo>() as i32,
            "process ancestry is unavailable ({})",
            std::io::Error::last_os_error()
        );
        // SAFETY: the matching flavor returned the complete fixed-size buffer.
        // Its integer fields and arrays accept all bit patterns.
        Ok(unsafe { info.assume_init() })
    }
    pub(super) fn path(pid: u32) -> Result<String> {
        let pid: i32 = pid.try_into().context("invalid process PID")?;
        let mut buffer = [0u8; 4096];
        // SAFETY: this writable buffer has PROC_PIDPATHINFO_MAXSIZE bytes and
        // stays alive for the synchronous call, which retains no pointer.
        let count =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        ensure!(
            count > 0 && count < buffer.len() as i32,
            "process executable path is unavailable"
        );
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .context("invalid executable path")?;
        let path = std::str::from_utf8(&buffer[..end]).context("executable path is not UTF-8")?;
        ensure!(
            Path::new(path).is_absolute() && Path::new(path).is_file(),
            "process executable is unavailable"
        );
        Ok(path.to_owned())
    }
    pub(super) fn capture(pid: u32) -> Result<ProcessIdentity> {
        ensure!(pid > 1 && pid <= i32::MAX as u32, "invalid process PID");
        let before = bsd_unique_info(pid)?;
        let path = path(pid)?;
        let after = bsd_unique_info(pid)?;
        ensure!(
            before.bsd.pbi_pid == pid
                && after.bsd.pbi_pid == pid
                && before.bsd.pbi_uid == after.bsd.pbi_uid
                && before.bsd.pbi_start_tvsec == after.bsd.pbi_start_tvsec
                && before.bsd.pbi_start_tvusec == after.bsd.pbi_start_tvusec
                && before.unique.unique_id == after.unique.unique_id
                && before.unique.pid_version == after.unique.pid_version,
            "process changed while capturing its identity"
        );
        let bundle_path =
            verified_bundle(Path::new(&path)).map(|bundle| bundle.to_string_lossy().into_owned());
        Ok(ProcessIdentity {
            pid,
            uid: after.bsd.pbi_uid,
            parent_pid: after.bsd.pbi_ppid,
            start_sec: after.bsd.pbi_start_tvsec,
            start_usec: after.bsd.pbi_start_tvusec,
            pid_version: after.unique.pid_version as u32,
            path,
            bundle_path,
        })
    }
    pub(super) fn resource_identity_matches(identity: &ProcessIdentity) -> Result<bool> {
        let current = bsd_unique_info(identity.pid)?;
        // SAFETY: geteuid takes no pointers and has no caller preconditions.
        let uid = unsafe { libc::geteuid() };
        Ok(current.bsd.pbi_pid == identity.pid
            && current.bsd.pbi_uid == identity.uid
            && current.bsd.pbi_uid == uid
            && current.bsd.pbi_start_tvsec == identity.start_sec
            && current.bsd.pbi_start_tvusec == identity.start_usec
            && current.unique.pid_version as u32 == identity.pid_version)
    }
    pub(super) fn resource_pids(limit: usize) -> Result<(Vec<u32>, bool)> {
        let capacity = limit.checked_add(1).context("process limit is too large")?;
        let bytes = capacity
            .checked_mul(size_of::<i32>())
            .and_then(|bytes| i32::try_from(bytes).ok())
            .context("process enumeration buffer is too large")?;
        let mut pids = vec![0i32; capacity];
        // SAFETY: the initialized i32 buffer is aligned, writable for exactly
        // bytes bytes, and alive for this synchronous call. PROC_UID_ONLY (4)
        // returns integer PIDs; neither function retains a pointer. geteuid has
        // no caller preconditions.
        let count =
            unsafe { libc::proc_listpids(4, libc::geteuid(), pids.as_mut_ptr().cast(), bytes) };
        ensure!(
            count >= 0 && count <= bytes && (count as usize).is_multiple_of(size_of::<i32>()),
            "process enumeration failed"
        );
        // A zero-length result is an error: this process itself belongs to the user.
        ensure!(count > 0, "process enumeration is unavailable");
        pids.truncate(count as usize / size_of::<i32>());
        let limited = count == bytes || pids.len() > limit;
        let pids = pids
            .into_iter()
            .filter(|pid| *pid > 1)
            .take(limit)
            .map(|pid| pid as u32)
            .collect();
        Ok((pids, limited))
    }
    impl ProcessSystem for Native {
        fn uid(&self) -> u32 {
            // SAFETY: geteuid takes no pointers and has no caller preconditions.
            unsafe { libc::geteuid() }
        }
        fn own_pid(&self) -> u32 {
            std::process::id()
        }
        fn parent_pid(&mut self, pid: u32) -> Result<u32> {
            let info = short_bsd_info(pid)?;
            ensure!(info.pbsi_pid == pid, "ancestor process identity changed");
            Ok(info.pbsi_ppid)
        }
        #[cfg(test)]
        fn list_pids(&mut self) -> Result<Vec<u32>> {
            let mut pids = vec![0i32; MAX_PROCESSES];
            let bytes = (pids.len() * size_of::<i32>()) as i32;
            // SAFETY: MAX_PROCESSES bounds bytes within i32. The initialized,
            // aligned i32 buffer is writable for bytes bytes and alive for this
            // synchronous call; PROC_UID_ONLY (4) retains no pointer.
            let count =
                unsafe { libc::proc_listpids(4, self.uid(), pids.as_mut_ptr().cast(), bytes) };
            ensure!(
                count > 0 && count < bytes && (count as usize).is_multiple_of(size_of::<i32>()),
                "process enumeration failed or exceeded its safety limit"
            );
            pids.truncate(count as usize / size_of::<i32>());
            Ok(pids
                .into_iter()
                .filter(|pid| *pid > 1)
                .map(|pid| pid as u32)
                .collect())
        }
        fn capture(&mut self, pid: u32) -> Result<ProcessIdentity> {
            capture(pid)
        }
        fn signal(&mut self, identity: &ProcessIdentity, mode: TerminationMode) -> Result<()> {
            // Resolve the API at runtime so older systems fail closed, rather than
            // reverting to kill(pid), which cannot bind the signal to a generation.
            let name: &CStr = c"proc_signal_with_audittoken";
            // SAFETY: RTLD_DEFAULT is the supported global lookup handle and
            // name is a static, NUL-terminated string valid during the call.
            let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
            ensure!(
                !symbol.is_null(),
                "identity-bound process signaling is unavailable on this macOS version"
            );
            // SAFETY: the non-null system libproc symbol has the SDK signature
            // int (audit_token_t *, int). AuditToken has audit_token_t's C layout;
            // macOS supports dlsym function-pointer conversion. The system
            // library remains loaded throughout this call.
            let signal: AuditSignal = unsafe { std::mem::transmute(symbol) };
            let mut token = AuditToken { values: [0; 8] };
            token.values[1] = identity.uid;
            token.values[5] = identity.pid;
            token.values[7] = identity.pid_version;
            let number = match mode {
                TerminationMode::Terminate => libc::SIGTERM,
                TerminationMode::ForceKill => libc::SIGKILL,
            };
            // SAFETY: signal has the verified ABI and receives a fully initialized,
            // aligned token alive for the synchronous call; it retains no pointer.
            // XNU resolves and holds the exact PID/version (slots 5/7), then checks
            // the caller's permissions. Other token fields do not grant authority.
            let error = unsafe { signal(&mut token, number) };
            ensure!(
                error == 0,
                "signal was not delivered to PID {} ({})",
                identity.pid,
                std::io::Error::from_raw_os_error(error)
            );
            Ok(())
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn typed_identity_readers_agree_on_current_process() {
            let pid = std::process::id();
            let unique = bsd_unique_info(pid).unwrap();
            let short = short_bsd_info(pid).unwrap();
            assert_eq!(unique.bsd.pbi_pid, pid);
            assert_eq!(short.pbsi_pid, pid);
            assert_eq!(unique.bsd.pbi_uid, short.pbsi_uid);
            assert_eq!(unique.bsd.pbi_ppid, short.pbsi_ppid);
            assert!(unique.bsd.pbi_start_tvsec > 0);
            assert!(bsd_unique_info(u32::MAX).is_err());
            assert!(short_bsd_info(u32::MAX).is_err());
        }

        #[test]
        fn enumeration_rejects_oversized_buffers_before_allocation() {
            assert!(resource_pids(usize::MAX).is_err());
            assert!(resource_pids(i32::MAX as usize).is_err());
            let (pids, _) = resource_pids(1).unwrap();
            assert!(pids.len() <= 1);
        }
    }
}
#[cfg(target_os = "macos")]
pub(crate) fn resource_identity_matches(identity: &ProcessIdentity) -> Result<bool> {
    macos::resource_identity_matches(identity)
}
#[cfg(target_os = "macos")]
pub(crate) fn resource_pids(limit: usize) -> Result<(Vec<u32>, bool)> {
    macos::resource_pids(limit)
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn resource_identity_matches(_: &ProcessIdentity) -> Result<bool> {
    anyhow::bail!("process identity capture requires macOS")
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn resource_pids(_: usize) -> Result<(Vec<u32>, bool)> {
    anyhow::bail!("process enumeration requires macOS")
}
#[cfg(target_os = "macos")]
pub(super) fn capture(pid: u32) -> Result<ProcessIdentity> {
    macos::capture(pid)
}
#[cfg(target_os = "macos")]
pub(super) fn process_path(pid: u32) -> Option<String> {
    macos::path(pid).ok()
}
#[cfg(not(target_os = "macos"))]
pub(super) fn capture(_: u32) -> Result<ProcessIdentity> {
    anyhow::bail!("process identity capture requires macOS")
}
#[cfg(not(target_os = "macos"))]
pub(super) fn process_path(_: u32) -> Option<String> {
    None
}
#[cfg(not(target_os = "macos"))]
impl ProcessSystem for Native {
    fn uid(&self) -> u32 {
        0
    }
    fn own_pid(&self) -> u32 {
        std::process::id()
    }
    fn parent_pid(&mut self, _: u32) -> Result<u32> {
        anyhow::bail!("process identity capture requires macOS")
    }
    #[cfg(test)]
    fn list_pids(&mut self) -> Result<Vec<u32>> {
        anyhow::bail!("process enumeration requires macOS")
    }
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity> {
        capture(pid)
    }
    fn signal(&mut self, _: &ProcessIdentity, _: TerminationMode) -> Result<()> {
        anyhow::bail!("process signaling requires macOS")
    }
}
