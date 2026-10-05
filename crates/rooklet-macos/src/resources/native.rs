use super::engine::{Counters, ResourceSystem};
use crate::process;
use anyhow::Result;
use rooklet_core::process::ProcessIdentity;
use std::time::{Duration, Instant};

pub(super) struct Native {
    started: Instant,
    #[cfg(target_os = "macos")]
    nanoseconds_per_tick: f64,
}
impl Default for Native {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            #[cfg(target_os = "macos")]
            nanoseconds_per_tick: timebase(),
        }
    }
}
#[cfg(target_os = "macos")]
fn timebase() -> f64 {
    // Fixed ABI from the SDK's mach/mach_time.h; avoid libc's deprecated
    // Mach Rust wrappers without adding another dependency for one system call.
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        #[link_name = "mach_timebase_info"]
        fn native_timebase_info(info: *mut Timebase) -> libc::c_int;
    }
    let mut info = Timebase { numer: 0, denom: 0 };
    let result = unsafe { native_timebase_info(&mut info) };
    if result == 0 && info.numer > 0 && info.denom > 0 {
        f64::from(info.numer) / f64::from(info.denom)
    } else {
        f64::NAN
    }
}
impl ResourceSystem for Native {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
    fn enumerate(&mut self, limit: usize) -> Result<(Vec<u32>, bool)> {
        process::native::resource_pids(limit)
    }
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity> {
        let identity = process::capture(pid)?;
        #[cfg(target_os = "macos")]
        anyhow::ensure!(
            identity.uid != 0 && identity.uid == unsafe { libc::geteuid() },
            "process is not owned by the current non-root user"
        );
        Ok(identity)
    }
    fn matches(&mut self, identity: &ProcessIdentity) -> Result<bool> {
        process::native::resource_identity_matches(identity)
    }
    #[cfg(target_os = "macos")]
    fn counters(&mut self, pid: u32) -> Result<Counters> {
        use anyhow::{Context, ensure};
        use std::mem::MaybeUninit;
        ensure!(
            self.nanoseconds_per_tick.is_finite(),
            "CPU timebase unavailable"
        );
        let pid = i32::try_from(pid).context("invalid process PID")?;
        let mut info = MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // libproc accepts a pointer to the fixed rusage buffer, cast to its
        // historical rusage_info_t pointer signature. A failure never exposes it.
        let result =
            unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
        ensure!(
            result == 0,
            "resource reading unavailable ({})",
            std::io::Error::last_os_error()
        );
        let info = unsafe { info.assume_init() };
        // XNU fill_task_rusage copies task_power_info total_user/total_system.
        // task_power_info_locked uses Mach absolute ticks (rm_time_mach), not
        // nanoseconds. Convert with mach_timebase_info when computing deltas.
        // https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c
        // https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c
        Ok(Counters {
            user_ticks: info.ri_user_time,
            system_ticks: info.ri_system_time,
            nanoseconds_per_tick: self.nanoseconds_per_tick,
            memory: info.ri_phys_footprint,
            read: info.ri_diskio_bytesread,
            write: info.ri_diskio_byteswritten,
        })
    }
    #[cfg(not(target_os = "macos"))]
    fn counters(&mut self, _: u32) -> Result<Counters> {
        anyhow::bail!("resource observations require macOS")
    }
}
