//! `global_logger!` expands, links as the `defmt` global logger, and `defmt` macros end up in its ring.
//! `error!`: the other levels are compiled out unless `DEFMT_LOG` says otherwise.

ww_defmt_logger::global_logger!(LOG, 256);
defmt::timestamp!("{=u32}", 7);

#[test]
fn defmt_macros_log_into_the_ring() {
    assert_eq!(LOG.pending_bytes(), 0);
    defmt::error!("hello {}", 42u8);
    defmt::error!("second");
    assert!(LOG.pending_bytes() > 2 * ww_defmt_logger::FRAME_OVERHEAD);
    assert_eq!(LOG.dropped_frames(), 0);
}
