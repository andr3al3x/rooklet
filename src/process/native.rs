//! macOS libproc adapter. No PID-only signaling fallback is allowed.
use super::engine::ProcessSystem;
use super::*;

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
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidpath(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
    }
    fn info<T>(pid: u32, flavor: i32) -> Result<T> {
        let pid: i32 = pid.try_into().context("invalid process PID")?;
        let mut info = MaybeUninit::<T>::zeroed();
        // The OS writes exactly this fixed ABI buffer; a short/failed read is rejected.
        let count = unsafe {
            libc::proc_pidinfo(
                pid,
                flavor,
                0,
                info.as_mut_ptr().cast(),
                size_of::<T>() as i32,
            )
        };
        ensure!(
            count == size_of::<T>() as i32,
            "process identity is unavailable ({})",
            std::io::Error::last_os_error()
        );
        Ok(unsafe { info.assume_init() })
    }
    pub(super) fn path(pid: u32) -> Result<String> {
        let pid: i32 = pid.try_into().context("invalid process PID")?;
        let mut buffer = [0u8; 4096];
        let count = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
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
        let before: BsdUniqueInfo = info(pid, BSD_UNIQUE)?;
        let path = path(pid)?;
        let after: BsdUniqueInfo = info(pid, BSD_UNIQUE)?;
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
    impl ProcessSystem for Native {
        fn uid(&self) -> u32 {
            unsafe { libc::geteuid() }
        }
        fn own_pid(&self) -> u32 {
            std::process::id()
        }
        fn parent_pid(&mut self, pid: u32) -> Result<u32> {
            let info: libc::proc_bsdshortinfo = info(pid, SHORT_BSD)?;
            ensure!(info.pbsi_pid == pid, "ancestor process identity changed");
            Ok(info.pbsi_ppid)
        }
        fn list_pids(&mut self) -> Result<Vec<u32>> {
            let mut pids = vec![0i32; MAX_PROCESSES];
            let bytes = (pids.len() * size_of::<i32>()) as i32;
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
            let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
            ensure!(
                !symbol.is_null(),
                "identity-bound process signaling is unavailable on this macOS version"
            );
            let signal: AuditSignal = unsafe { std::mem::transmute(symbol) };
            let mut token = AuditToken { values: [0; 8] };
            token.values[1] = identity.uid;
            token.values[5] = identity.pid;
            token.values[7] = identity.pid_version;
            let number = match mode {
                TerminationMode::Terminate => libc::SIGTERM,
                TerminationMode::ForceKill => libc::SIGKILL,
            };
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
