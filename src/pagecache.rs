//! Keep the container's page cache out of the cgroup memory limit.
//!
//! The worker runs the operator with a hard `--memory` limit (`exe_runner.dart`), and a cgroup
//! counts **page cache** against that limit, not just anonymous memory. This operator is
//! I/O-heavy by nature: importing the 93-file spectral cohort writes the 1.28 GB archive, then
//! the 1.2 GB of extracted FCS files, then the result — about 3 GB of file pages against an RSS
//! that never exceeds 854 MB. Reclaim usually keeps up, but a fast writer produces dirty pages
//! faster than writeback retires them, and the cgroup OOM-kills the process: measured as exit 137
//! under a 1800 MB limit on 2026-09-19, with the process itself using well under half of it.
//!
//! So every large file this operator writes or reads is handed back to the kernel once it is no
//! longer needed: `fsync` to retire the dirty pages, then `posix_fadvise(POSIX_FADV_DONTNEED)` to
//! drop the clean ones. Both are advisory and best-effort; failures are ignored on purpose,
//! since the operator is still correct (just fatter) without them.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsRawFd;

const POSIX_FADV_DONTNEED: i32 = 4;

unsafe extern "C" {
    fn posix_fadvise(fd: i32, offset: i64, len: i64, advice: i32) -> i32;
}

/// Flush `f` to disk and drop its pages from the cache. Best-effort.
pub fn release(f: &File) {
    let _ = f.sync_all();
    unsafe {
        posix_fadvise(f.as_raw_fd(), 0, 0, POSIX_FADV_DONTNEED);
    }
}

/// Drop the cached pages of a path without holding it open. Best-effort.
pub fn release_path(p: &std::path::Path) {
    if let Ok(f) = File::open(p) {
        release(&f);
    }
}

/// A writer that releases what it has written every `every` bytes, so writing a multi-GB result
/// does not accumulate GBs of page cache inside the cgroup.
pub struct Releasing<W: Write + AsRawFd> {
    inner: W,
    since: u64,
    every: u64,
}

impl<W: Write + AsRawFd> Releasing<W> {
    pub fn new(inner: W, every: u64) -> Self {
        Self {
            inner,
            since: 0,
            every,
        }
    }
    fn maybe_release(&mut self) {
        if self.since >= self.every {
            self.since = 0;
            unsafe {
                // fsync via the raw fd: the pages must be clean before they can be dropped.
                libc_fsync(self.inner.as_raw_fd());
                posix_fadvise(self.inner.as_raw_fd(), 0, 0, POSIX_FADV_DONTNEED);
            }
        }
    }
}

unsafe extern "C" {
    #[link_name = "fsync"]
    fn libc_fsync(fd: i32) -> i32;
}

impl<W: Write + AsRawFd> Write for Releasing<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.since += n as u64;
        self.maybe_release();
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
