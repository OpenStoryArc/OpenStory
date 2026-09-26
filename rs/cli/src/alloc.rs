//! B-06: the allocator this binary runs (docs/research/openstory-as-node/
//! 2026-09-25-boot-pass-memory.md). jemalloc behind the default-on
//! `jemalloc` feature, tuned to give freed pages back within a second
//! (`background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000`);
//! `--no-default-features` keeps the platform allocator.

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// jemalloc reads its options from this symbol at first use (the crate
/// prefixes jemalloc's symbols, hence `_rjem_`). A background thread
/// returns dirty and muzzy pages within a second of their last use
/// instead of the 10 s / 0 s defaults, so the settle-after-serving RSS
/// falls back toward what is live. Environment `MALLOC_CONF` still
/// overrides this at run time.
#[cfg(feature = "jemalloc")]
#[allow(non_upper_case_globals)]
#[export_name = "_rjem_malloc_conf"]
pub static malloc_conf: &[u8] = b"background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000\0";

/// The name health reports as `process.allocator`.
pub fn describe() -> &'static str {
    if cfg!(feature = "jemalloc") {
        "jemalloc"
    } else {
        "system"
    }
}

/// jemalloc's effective options, read back through mallctl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub background_thread: bool,
    pub dirty_decay_ms: i64,
    pub muzzy_decay_ms: i64,
}

#[cfg(feature = "jemalloc")]
pub fn options() -> Option<Options> {
    use tikv_jemalloc_ctl::raw;
    // Safety: these are the documented mallctl keys with their documented
    // types (bool, ssize_t, ssize_t); a wrong type is a read error, not UB.
    let background_thread = unsafe { raw::read::<bool>(b"opt.background_thread\0") }.ok()?;
    let dirty_decay_ms = unsafe { raw::read::<isize>(b"opt.dirty_decay_ms\0") }.ok()?;
    let muzzy_decay_ms = unsafe { raw::read::<isize>(b"opt.muzzy_decay_ms\0") }.ok()?;
    Some(Options {
        background_thread,
        dirty_decay_ms: dirty_decay_ms as i64,
        muzzy_decay_ms: muzzy_decay_ms as i64,
    })
}

#[cfg(not(feature = "jemalloc"))]
pub fn options() -> Option<Options> {
    None
}

/// B-06: `describe()` names the allocator and `options()` reads jemalloc's
/// effective decay settings, so a build cannot silently ship glibc or the
/// wrong MALLOC_CONF.
#[cfg(test)]
mod specs {
    #[cfg(feature = "jemalloc")]
    mod when_jemalloc_is_the_allocator {
        #[test]
        fn it_names_itself_and_runs_with_the_decay_config() {
            assert_eq!(super::super::describe(), "jemalloc");
            let opts = super::super::options().expect("jemalloc answers mallctl");
            assert!(opts.background_thread, "background_thread:true");
            assert_eq!(opts.dirty_decay_ms, 1000);
            assert_eq!(opts.muzzy_decay_ms, 1000);
        }
    }

    #[cfg(not(feature = "jemalloc"))]
    mod when_the_system_allocator_is_kept {
        #[test]
        fn it_says_system_and_has_no_options() {
            assert_eq!(super::super::describe(), "system");
            assert!(super::super::options().is_none());
        }
    }
}
