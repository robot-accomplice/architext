//! PID liveness + stop escalation for the serve lifecycle.
//!
//! Port of `pidExists` / `waitForPidGone` / `stopServeProcess` from
//! `src/adapters/cli/serve-lifecycle.mjs` (lines 132-140, 380-412).
//!
//! CRITICAL — the RCA fix (serve-lifecycle.mjs:389-411): a serve refresh
//! re-spawns on the OLD child's EXACT port, so the stop MUST NOT return until
//! the pid is genuinely gone. Under load a child may not be scheduled to act on
//! SIGTERM within the stop window (and the caller's own poll loop is itself
//! starved), so a fixed wall-clock SIGTERM wait can expire while the child is
//! merely descheduled. SIGKILL is delivered by the kernel regardless of
//! scheduling, so we escalate SIGTERM → wait → SIGKILL → wait-until-gone and
//! confirm death before returning, guaranteeing the port is free for re-spawn.

use std::time::{Duration, Instant};

// Stop-escalation constants (mirror serveStopTimeoutMs / serveStopKillTimeoutMs
// / serveStopPollMs in serve-lifecycle.mjs:31-33).
pub const SERVE_STOP_TIMEOUT_MS: u64 = 3000;
pub const SERVE_STOP_KILL_TIMEOUT_MS: u64 = 5000;
pub const SERVE_STOP_POLL_MS: u64 = 100;

/// Port of `pidExists`: `process.kill(pid, 0)` returns true iff the process
/// exists (signal 0 is a liveness probe, delivers nothing).
#[cfg(unix)]
pub fn pid_exists(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    // kill(pid, None) sends signal 0 — Ok means alive (or zombie we can signal),
    // Err(ESRCH) means gone, Err(EPERM) means alive but not ours (still "exists").
    match kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
pub fn pid_exists(_pid: i64) -> bool {
    false
}

#[cfg(unix)]
fn send_signal(pid: i64, sig: nix::sys::signal::Signal) -> Result<(), ()> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), sig).map_err(|_| ())
}

/// Port of `waitForPidGone`: poll until the pid disappears or the timeout
/// elapses. Returns `true` if the pid is gone.
pub fn wait_for_pid_gone(pid: i64, timeout_ms: u64, poll_ms: u64) -> bool {
    let started = Instant::now();
    while pid_exists(pid) {
        if started.elapsed() > Duration::from_millis(timeout_ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(poll_ms));
    }
    true
}

/// Stop a serve child and DO NOT return until it is genuinely gone.
///
/// SIGTERM first for a clean exit; if the pid survives the term window, escalate
/// to SIGKILL and confirm death before returning. Returns `true` once the pid is
/// gone, `false` only if even SIGKILL did not clear it within the kill window.
///
/// Port of `stopServeProcess(pid, { termTimeoutMs, killTimeoutMs, pollMs })`.
#[cfg(unix)]
pub fn stop_serve_process(pid: i64, term_timeout_ms: u64, kill_timeout_ms: u64, poll_ms: u64) -> bool {
    use nix::sys::signal::Signal;

    if !pid_exists(pid) {
        return true;
    }
    if send_signal(pid, Signal::SIGTERM).is_err() {
        // Exited between the liveness check and the signal.
        return true;
    }
    if wait_for_pid_gone(pid, term_timeout_ms, poll_ms) {
        return true;
    }
    if send_signal(pid, Signal::SIGKILL).is_err() {
        // Exited after the SIGTERM wait but before the escalation.
        return true;
    }
    wait_for_pid_gone(pid, kill_timeout_ms, poll_ms)
}

#[cfg(not(unix))]
pub fn stop_serve_process(_pid: i64, _term_timeout_ms: u64, _kill_timeout_ms: u64, _poll_ms: u64) -> bool {
    true
}

/// Convenience wrapper with the default stop-escalation constants.
pub fn stop_serve_process_default(pid: i64) -> bool {
    stop_serve_process(pid, SERVE_STOP_TIMEOUT_MS, SERVE_STOP_KILL_TIMEOUT_MS, SERVE_STOP_POLL_MS)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use nix::sys::signal::Signal;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    /// The RCA's deterministic test: a child that IGNORES SIGTERM must still be
    /// stopped — `stop_serve_process` returns only after the pid is gone, which
    /// for a SIGTERM-ignoring child can only happen via SIGKILL.
    ///
    /// RED without the SIGKILL escalation (the function would return `false`
    /// after the term window with the pid still alive); GREEN with it (returns
    /// `true` after SIGKILL clears the pid, within the kill window).
    #[test]
    fn stop_escalates_to_sigkill_when_sigterm_ignored() {
        // `trap '' TERM` makes the shell ignore SIGTERM; `exec sleep 600` keeps
        // the SAME pid alive (exec replaces the shell, so there is no separate
        // sleep child to reparent) so only SIGKILL can clear it.
        //
        // In production the serve child is detached and reparented to init, so a
        // SIGKILL'd pid is reaped immediately and `pid_exists` sees it gone. In
        // a unit test the child is OUR direct child, so after SIGKILL it lingers
        // as a zombie (kill(pid,0) on a zombie still returns Ok). We model the
        // production "reaped by init" behavior with a reaper thread that calls
        // `wait()` once the process dies, clearing the zombie so `pid_exists`
        // can observe the pid genuinely gone.
        let child = Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; exec sleep 600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn SIGTERM-ignoring child");
        let pid = child.id() as i64;

        // Reaper: wait() in the background so the zombie is cleared right after
        // the kernel delivers SIGKILL (mirrors init reaping a detached child).
        let reaper = std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });

        // Give the shell a moment to install the trap + exec sleep so SIGTERM is
        // genuinely ignored rather than racing the trap setup.
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(pid_exists(pid), "child should be alive before stop");

        // Short term window (so the test is fast) — the child ignores SIGTERM,
        // so the only way `stopped` is true is the SIGKILL escalation firing.
        let term_ms = 400;
        let kill_ms = 5000;
        let started = Instant::now();
        let stopped = stop_serve_process(pid, term_ms, kill_ms, 50);
        let elapsed = started.elapsed();

        // ALWAYS clean up: if the escalation is missing (RED), the child is still
        // alive and the reaper is blocked, so force a SIGKILL here so the reaper
        // unblocks and the test process can exit cleanly instead of hanging. This
        // makes the RED case a clean assertion FAILURE, not a hang.
        let _ = send_signal(pid, Signal::SIGKILL);
        let _ = reaper.join();

        assert!(stopped, "stop_serve_process must SIGKILL a SIGTERM-ignoring child and confirm death");
        // Must have waited past the term window (proves SIGTERM did not clear it)
        // but well within the kill window.
        assert!(
            elapsed >= std::time::Duration::from_millis(term_ms),
            "stop returned before the SIGTERM window elapsed ({elapsed:?}) — SIGTERM cannot have cleared a trap-ignoring child"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(term_ms + kill_ms),
            "stop took too long: {elapsed:?}"
        );
    }

    #[test]
    fn pid_exists_false_for_dead_pid() {
        // Spawn + reap a child, then its pid should not exist.
        let mut child = Command::new("true").spawn().expect("spawn true");
        let pid = child.id() as i64;
        let _ = child.wait();
        // After reaping, the pid is gone.
        assert!(!pid_exists(pid), "reaped pid should not exist");
    }

    #[test]
    fn stop_returns_true_for_already_dead_pid() {
        let mut child = Command::new("true").spawn().expect("spawn true");
        let pid = child.id() as i64;
        let _ = child.wait();
        assert!(stop_serve_process_default(pid), "stopping a dead pid is a no-op success");
    }
}
