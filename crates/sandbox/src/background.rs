//! Background work's threads (M1c.md, step 8): pools of their own at a
//! lower priority than the view's, so that drawing and the UI's rebuilds
//! (on rayon's global pool, which runs a task to its end once started)
//! take the cores they need while every core designs or reads maps.

/// How much their threads' nice value is above the thread's that starts
/// the pool (Linux; 19 at most). At the view's priority, a ship's lines
/// built in 1–2 ms took 33–67 ms while every core designed.
const NICE: i32 = 10;

/// A pool of a thread per core, named `name` and a number, [`NICE`] below
/// the starting thread's priority where a thread's priority is its own
/// (Linux); elsewhere, at its priority for now.
pub fn pool(name: &'static str) -> rayon::ThreadPool {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(move |i| format!("{name} {i}"))
        .start_handler(|_| lower())
        .build()
        .expect("a thread pool")
}

/// On a new pool thread, which starts at its starting thread's nice.
fn lower() {
    #[cfg(target_os = "linux")]
    // SAFETY: no memory is touched; `who` 0 is the calling thread. (An
    // error reads as -1, and leaves the thread at nice 9.)
    unsafe {
        let own = libc::getpriority(libc::PRIO_PROCESS, 0);
        libc::setpriority(libc::PRIO_PROCESS, 0, (own + NICE).min(19));
    }
}
