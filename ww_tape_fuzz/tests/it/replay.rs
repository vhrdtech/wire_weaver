//! Every seed file of every target through its library function.

use crate::Times;
use crate::known::{KNOWN, is_known};
use crate::seeds::seed_files;
use ww_tape_fuzz::case::run;
use ww_tape_fuzz::targets;

/// Replay every seed of `target`: no failure but the known ones, and every known one still fails where it is known.
fn replay(target: &str) {
    let f = targets::by_name(target).unwrap();
    let files = seed_files(target);
    let mut times = Times::default();
    let mut failures = vec![];
    for (name, bytes) in &files {
        let (outcome, took) = run(f, bytes);
        times.add(took, || name.clone());
        if outcome.place().is_some() && !is_known(target, &outcome) {
            failures.push(format!("{name}: {outcome:?}"));
        }
        for k in KNOWN
            .iter()
            .filter(|k| k.target == target && k.seed == name)
        {
            assert_eq!(
                outcome.place(),
                Some(k.at),
                "{target}/{name} no longer fails at {} ({}): fixed? then its entry in known.rs goes",
                k.at,
                k.message
            );
        }
    }
    times.report(target);
    for k in KNOWN.iter().filter(|k| k.target == target) {
        assert!(
            files.iter().any(|(n, _)| n == k.seed),
            "{target}: the seed {} of a known failure is missing",
            k.seed
        );
    }
    assert!(failures.is_empty(), "{target}:\n{}", failures.join("\n"));
}

macro_rules! replays {
    ($($test:ident: $name:literal;)*) => {$(
        #[test]
        fn $test() {
            replay($name);
        }
    )*};
}

replays! {
    replay_tape_chunk: "tape-chunk";
    replay_tape_index: "tape-index";
    replay_tape_payloads: "tape-payloads";
    replay_tape_reader: "tape-reader";
    replay_tape_resume: "tape-resume";
    replay_tape_samples: "tape-samples";
    replay_tape_series: "tape-series";
    replay_tape_summary: "tape-summary";
    replay_tape_times: "tape-times";
    replay_tape_chunk_hostile: "tape-chunk-hostile";
    replay_tape_index_hostile: "tape-index-hostile";
    replay_tape_payloads_hostile: "tape-payloads-hostile";
    replay_tape_reader_hostile: "tape-reader-hostile";
    replay_tape_records_hostile: "tape-records-hostile";
    replay_tape_samples_hostile: "tape-samples-hostile";
    replay_tape_series_hostile: "tape-series-hostile";
    replay_tape_summary_hostile: "tape-summary-hostile";
    replay_tape_times_hostile: "tape-times-hostile";
}

/// Every target has a replay test above.
#[test]
fn every_target_replays() {
    let source = include_str!("replay.rs");
    for (name, _) in targets::ALL {
        assert!(
            source.contains(&format!("\"{name}\";")),
            "{name} has no replay test"
        );
    }
}
