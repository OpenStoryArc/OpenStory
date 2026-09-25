//! B-06: the allocator this binary runs (docs/research/openstory-as-node/
//! 2026-09-25-boot-pass-memory.md). jemalloc behind the default-on
//! `jemalloc` feature, tuned to give freed pages back within a second
//! (`background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000`);
//! `--no-default-features` keeps the platform allocator.

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
