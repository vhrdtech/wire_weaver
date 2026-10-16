//! [SeriesColumns]: how a type writes and reads its group of columns in a [Series](super::Series), and the pieces
//! its impls are made of (by hand here for the primitives, by `#[derive_shrink_wrap(.., series)]` for user types).
//!
//! A group covers a subsequence of the `Series`' items: all of them at the top, fewer inside enum variants and
//! `Option`s. Writers get the subsequence as an iterator of `(&item, restart)` and walk it once per column. The
//! reading side is two passes over the same rules:
//!
//! - [skim](SeriesColumns::skim) finds where every column starts and validates it, in column order. A column's
//!   item count and restarts come from the runs before it, which a [SeriesCtx] re-walks with their own cursors,
//!   one step per item of the `Series`: no allocation, work bounded by the items times the columns.
//! - [next](SeriesColumns::next) yields one item from the column cursors the skim left at the column starts,
//!   stepping all of them in parallel; restarts come from the values just decoded.
//!
//! Restarts: a numeric or float column starts a new segment where its item enters an enum variant (or `Some`)
//! from another one, where an enum-typed field declared earlier in the same struct (tuple, array, variant)
//! changed, and wherever the enclosing group restarts. `Option` counts as an enum here; bools, strings and opaque
//! fields restart nothing.

use super::BitSink;
use super::columns::run::{RunKind, RunReader};
use super::columns::{get_nib, put_nib};
use crate::{BufReader, Error};
use core::marker::PhantomData;

pub use super::columns::CodecPolicy;

/// Where a group sits, for what an evolvable struct writes before its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Level {
    /// `T` of the `Series` itself: an evolvable struct writes its field count (`nib`) first.
    Outermost,
    /// Anywhere inside: an evolvable struct writes the bits of its group after the length (`nib`) first.
    Nested,
    /// Only the fields' groups, no count or length: what enum variants write, and how an evolvable struct
    /// measures its own group.
    Fields,
}

/// One item of the `Series` as a group sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Slot {
    /// Not in this group's subsequence (another enum variant, a `None`).
    Absent,
    /// In it, and whether its columns restart here.
    Present { restart: bool },
}

/// The subsequence of a group during [skim](SeriesColumns::skim): steps through all `n` items of the `Series`,
/// saying for each whether it is in the group and restarts it. Cloned to walk it again, once per column.
pub trait SeriesCtx: Clone {
    /// The next item, `None` after the last one of the `Series`.
    fn step(&mut self) -> Result<Option<Slot>, Error>;

    /// Walks a copy to the end: the number of items in the subsequence.
    fn present(&self) -> Result<usize, Error> {
        let mut ctx = self.clone();
        let mut m = 0;
        while let Some(slot) = ctx.step()? {
            if slot != Slot::Absent {
                m += 1;
            }
        }
        Ok(m)
    }

    /// Walks a copy to the end, calling `f` with the restart flag of each item in the subsequence.
    fn for_each_present(&self, mut f: impl FnMut(bool) -> Result<(), Error>) -> Result<(), Error> {
        let mut ctx = self.clone();
        while let Some(slot) = ctx.step()? {
            if let Slot::Present { restart } = slot {
                f(restart)?;
            }
        }
        Ok(())
    }
}

/// All `n` items of the `Series`, none restarting: the context of `T` itself.
#[derive(Clone, Copy, Debug)]
pub struct RootCtx {
    left: usize,
}

impl RootCtx {
    pub const fn new(n: usize) -> Self {
        RootCtx { left: n }
    }
}

impl SeriesCtx for RootCtx {
    fn step(&mut self) -> Result<Option<Slot>, Error> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        Ok(Some(Slot::Present { restart: false }))
    }
}

/// A run of enum tags (or `Option` tags) positioned at its first item: what a later column's context re-walks.
#[derive(Clone, Copy, Debug)]
pub struct TagRun<'i> {
    run: RunReader<'i, RunKind>,
}

impl TagRun<'_> {
    fn next(&mut self) -> Result<u32, Error> {
        self.run.next()
    }
}

/// The items of one variant of an enum (`tag`), from the parent's context and the enum's tag run: restarts where
/// an item enters the variant from another one, and where the parent restarts.
#[derive(Clone, Copy, Debug)]
pub struct VariantCtx<'i, C> {
    parent: C,
    run: Option<TagRun<'i>>,
    tag: u32,
    prev: Option<u32>,
}

impl<'i, C: SeriesCtx> VariantCtx<'i, C> {
    /// `run` is the enum's [EnumTags::tag_run], `None` only when the parent has no items.
    pub fn new(parent: C, run: Option<TagRun<'i>>, tag: u32) -> Self {
        VariantCtx {
            parent,
            run,
            tag,
            prev: None,
        }
    }
}

impl<C: SeriesCtx> SeriesCtx for VariantCtx<'_, C> {
    fn step(&mut self) -> Result<Option<Slot>, Error> {
        let Some(slot) = self.parent.step()? else {
            return Ok(None);
        };
        let Slot::Present { restart } = slot else {
            return Ok(Some(Slot::Absent));
        };
        let tag = self.run.as_mut().ok_or(Error::MalformedSeries)?.next()?;
        let entered = self.prev.is_some_and(|p| p != tag);
        self.prev = Some(tag);
        Ok(Some(if tag == self.tag {
            Slot::Present {
                restart: restart || entered,
            }
        } else {
            Slot::Absent
        }))
    }
}

/// The context of a field of a struct (tuple, array, variant) with `N` fields: the parent's, plus a restart
/// where the tag of an earlier enum-typed field changed. `runs` holds [SeriesColumns::tag_run] of the fields
/// skimmed so far (`None` for fields that are not enums), so field `j` sees only the `j` before it.
#[derive(Clone, Copy, Debug)]
pub struct SiblingCtx<'i, C, const N: usize> {
    parent: C,
    runs: [Option<TagRun<'i>>; N],
    prev: [Option<u32>; N],
}

impl<'i, C: SeriesCtx, const N: usize> SiblingCtx<'i, C, N> {
    pub fn new(parent: C, runs: [Option<TagRun<'i>>; N]) -> Self {
        SiblingCtx {
            parent,
            runs,
            prev: [None; N],
        }
    }
}

impl<C: SeriesCtx, const N: usize> SeriesCtx for SiblingCtx<'_, C, N> {
    fn step(&mut self) -> Result<Option<Slot>, Error> {
        let Some(slot) = self.parent.step()? else {
            return Ok(None);
        };
        let Slot::Present { mut restart } = slot else {
            return Ok(Some(Slot::Absent));
        };
        for (run, prev) in self.runs.iter_mut().zip(self.prev.iter_mut()) {
            if let Some(run) = run {
                let tag = run.next()?;
                restart |= prev.is_some_and(|p| p != tag);
                *prev = Some(tag);
            }
        }
        Ok(Some(Slot::Present { restart }))
    }
}

/// The reading side of [SiblingCtx] in [next](SeriesColumns::next): the tags of an item's fields as they are
/// decoded, against the previous item's.
#[derive(Clone, Copy, Debug)]
pub struct SiblingTags<const N: usize> {
    prev: [Option<u32>; N],
    changed: bool,
}

impl<const N: usize> SiblingTags<N> {
    pub const fn new() -> Self {
        SiblingTags {
            prev: [None; N],
            changed: false,
        }
    }

    /// Before the first field of an item.
    pub fn begin(&mut self) {
        self.changed = false;
    }

    /// The restart flag of the next field: the item's, or an earlier enum field changed.
    pub fn restart(&self, item_restart: bool) -> bool {
        item_restart || self.changed
    }

    /// After field `j`: its [series_tag](SeriesColumns::series_tag).
    pub fn record(&mut self, j: usize, tag: Option<u32>) {
        if let Some(tag) = tag {
            self.changed |= self.prev[j].is_some_and(|p| p != tag);
            self.prev[j] = Some(tag);
        }
    }
}

impl<const N: usize> Default for SiblingTags<N> {
    fn default() -> Self {
        SiblingTags::new()
    }
}

/// The items of one field of a struct (tuple, array, enum variant) for the field's
/// [write_group](SeriesColumns::write_group): `field` picks it from an item (`None` outside the variant), `earlier`
/// tells whether an enum-typed field before it changed from the previous item (compare
/// [series_tag](SeriesColumns::series_tag)s). Restarts where the parent's item restarts, where the item enters
/// the variant, and where an earlier enum field changed.
pub struct FieldItems<'a, P: 'a, F: 'a, I, G, E> {
    items: I,
    field: G,
    earlier: E,
    prev: Option<&'a P>,
    _field: PhantomData<&'a F>,
}

impl<'a, P, F, I, G, E> FieldItems<'a, P, F, I, G, E>
where
    I: Iterator<Item = (&'a P, bool)> + Clone,
    G: Fn(&'a P) -> Option<&'a F> + Clone,
    E: Fn(&'a P, &'a P) -> bool + Clone,
{
    pub fn new(items: I, field: G, earlier: E) -> Self {
        FieldItems {
            items,
            field,
            earlier,
            prev: None,
            _field: PhantomData,
        }
    }
}

impl<'a, P, F, I: Clone, G: Clone, E: Clone> Clone for FieldItems<'a, P, F, I, G, E> {
    fn clone(&self) -> Self {
        FieldItems {
            items: self.items.clone(),
            field: self.field.clone(),
            earlier: self.earlier.clone(),
            prev: self.prev,
            _field: PhantomData,
        }
    }
}

impl<'a, P, F, I, G, E> Iterator for FieldItems<'a, P, F, I, G, E>
where
    I: Iterator<Item = (&'a P, bool)> + Clone,
    G: Fn(&'a P) -> Option<&'a F> + Clone,
    E: Fn(&'a P, &'a P) -> bool + Clone,
{
    type Item = (&'a F, bool);

    fn next(&mut self) -> Option<(&'a F, bool)> {
        loop {
            let (item, restart) = self.items.next()?;
            let prev = self.prev.replace(item);
            let Some(field) = (self.field)(item) else {
                continue;
            };
            let restart = restart
                || prev.is_some_and(|p| (self.field)(p).is_none() || (self.earlier)(p, item));
            return Some((field, restart));
        }
    }
}

/// `earlier` of [FieldItems] for field `j`, from the [series_tag](SeriesColumns::series_tag)s of all fields of the
/// previous and the current item: whether an enum-typed field before `j` changed.
pub fn changed_before(prev: &[Option<u32>], cur: &[Option<u32>], j: usize) -> bool {
    prev[..j]
        .iter()
        .zip(&cur[..j])
        .any(|(p, c)| p.is_some() && c.is_some() && p != c)
}

/// `earlier` of [FieldItems] for a first field: nothing before it.
pub fn no_earlier<P>(_prev: &P, _cur: &P) -> bool {
    false
}

/// Writes the tag run of an enum (the general form, discriminants as `nib`). Nothing for no items.
pub fn write_enum_tags(
    sink: &mut impl BitSink,
    tags: impl Iterator<Item = u32>,
) -> Result<(), Error> {
    super::columns::run::write_run(sink, RunKind::Tag, tags)
}

/// The tag run of an enum (or `Option`) while items are read, and the tag of the previous item.
#[derive(Clone, Copy, Debug)]
pub struct EnumTags<'i> {
    run: Option<TagRun<'i>>,
    prev: Option<u32>,
}

impl<'i> EnumTags<'i> {
    /// Skims the tag run of an enum over `ctx`'s items and checks every tag with `check` (an unknown one is
    /// `EnumFutureVersionOrMalformedData`, a compiled-out one `EnumVariantDisabled`). Leaves `rd` after the run.
    pub fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: &C,
        check: impl Fn(u32) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        Self::skim_kind(rd, ctx, RunKind::Tag, check)
    }

    pub(crate) fn skim_kind<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: &C,
        kind: RunKind,
        check: impl Fn(u32) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        let m = ctx.present()?;
        if m == 0 {
            return Ok(EnumTags {
                run: None,
                prev: None,
            });
        }
        let run = RunReader::new(*rd, kind, m)?;
        let mut walk = run;
        for _ in 0..m {
            check(walk.next()?)?;
        }
        *rd = walk.finish()?;
        Ok(EnumTags {
            run: Some(TagRun { run }),
            prev: None,
        })
    }

    /// The run at its first item, for [VariantCtx] and [SiblingCtx].
    pub fn tag_run(&self) -> Option<TagRun<'i>> {
        self.run
    }

    /// The next item's tag, and whether it entered its variant from another one (a restart of the variant).
    pub fn next_tag(&mut self) -> Result<(u32, bool), Error> {
        let tag = self.run.as_mut().ok_or(Error::MalformedSeries)?.next()?;
        let entered = self.prev.is_some_and(|p| p != tag);
        self.prev = Some(tag);
        Ok((tag, entered))
    }
}

/// How much of a struct's group the data has, from [skim_struct_head]: an evolvable struct of an older writer has
/// fewer fields (the rest take their defaults), one of a newer writer more (skipped).
#[derive(Clone, Copy, Debug)]
pub enum StructExtent {
    /// Not evolvable, or no items: every field the reader knows.
    All,
    /// Outermost: the writer's field count.
    Count(u32),
    /// Nested: the bit position where the group ends.
    End(usize),
}

impl StructExtent {
    /// Whether field `j` (from 0) is in the data, asked right before skimming it.
    pub fn has_field(&self, rd: &BufReader<'_>, j: usize) -> bool {
        match *self {
            StructExtent::All => true,
            StructExtent::Count(count) => j < count as usize,
            StructExtent::End(end) => rd.bit_pos() < end,
        }
    }

    /// After the fields the reader knows (`known` of them): skips the columns of fields it doesn't know. A nested
    /// group read past its length is `MalformedSeries`.
    pub fn finish(self, rd: &mut BufReader<'_>, known: usize) -> Result<(), Error> {
        match self {
            StructExtent::All => Ok(()),
            // the outermost group is the last thing in the Series
            StructExtent::Count(count) if count as usize > known => rd.skip_rest(),
            StructExtent::Count(_) => Ok(()),
            StructExtent::End(end) => rd.skip_to_bit(end),
        }
    }
}

/// Reads what a struct's group has before its fields: the field count (outermost) or the group length (nested)
/// of an evolvable struct with items, nothing otherwise.
pub fn skim_struct_head<C: SeriesCtx>(
    rd: &mut BufReader<'_>,
    ctx: &C,
    level: Level,
    evolvable: bool,
) -> Result<StructExtent, Error> {
    if !evolvable || level == Level::Fields || ctx.present()? == 0 {
        return Ok(StructExtent::All);
    }
    let value = get_nib(rd)?;
    if level == Level::Outermost {
        return Ok(StructExtent::Count(value));
    }
    let end = rd.bit_pos() + value as usize;
    if value as usize > rd.bits_left() {
        return Err(Error::MalformedSeries);
    }
    Ok(StructExtent::End(end))
}

/// Writes what a struct's group has before its fields, for items present: the field count `fields` of an
/// evolvable struct at [Level::Outermost], or its group length at [Level::Nested], `body_bits(start)` being the
/// bits of its fields written from bit position `start` ([BitCounter](super::BitCounter::at) and
/// [Level::Fields]). The length is the shortest `nib` that fits; when byte-aligned fields make the length depend on
/// its own size, it takes one more group than it needs.
pub fn write_struct_head<S: BitSink>(
    sink: &mut S,
    level: Level,
    evolvable: bool,
    fields: u32,
    body_bits: impl Fn(usize) -> Result<usize, Error>,
) -> Result<(), Error> {
    if !evolvable {
        return Ok(());
    }
    match level {
        Level::Fields => Ok(()),
        Level::Outermost => put_nib(sink, fields),
        Level::Nested => {
            let start = sink.bit_pos();
            for groups in 1..=11u8 {
                let bits = body_bits(start + 4 * groups as usize)?;
                let bits = u32::try_from(bits).map_err(|_| Error::LenTooLong)?;
                if nib_groups(bits) <= groups {
                    return put_nib_groups(sink, bits, groups);
                }
            }
            Err(Error::LenTooLong)
        }
    }
}

/// Groups of 4 bits [put_nib] takes for `value`.
fn nib_groups(value: u32) -> u8 {
    let mut groups = 1;
    let mut value = value >> 3;
    while value != 0 {
        groups += 1;
        value >>= 3;
    }
    groups
}

/// `nib(value)` in exactly `groups` groups (at least [nib_groups]): the extra ones are `0b1000` / `0b0000`, which
/// [get_nib] reads as the same value.
fn put_nib_groups(sink: &mut impl BitSink, mut value: u32, groups: u8) -> Result<(), Error> {
    for g in 0..groups {
        let more = g + 1 < groups;
        sink.put_bits(4, (value & 0b111) as u64 | if more { 0b1000 } else { 0 })?;
        value >>= 3;
    }
    Ok(())
}

/// A type that can be the item type of a [Series](super::Series), or a field of one: writes and reads its group of
/// columns. Implemented here for the primitives, `Option`, tuples, arrays, strings and byte lists, and by
/// `#[derive_shrink_wrap(.., series)]` for structs and enums.
///
/// Every method gets the group's subsequence: an empty one writes and reads nothing at all.
pub trait SeriesColumns<'i>: Sized {
    /// Column cursors and codec state of all of `Self`'s columns.
    type Reader: Clone;

    /// Writes this type's group over `items` (the subsequence, with the restart flag of each), column by column,
    /// walking `items` once or more per column.
    fn write_group<'a, I, S>(
        items: I,
        sink: &mut S,
        policy: &CodecPolicy,
        level: Level,
    ) -> Result<(), Error>
    where
        I: Iterator<Item = (&'a Self, bool)> + Clone,
        S: BitSink,
        Self: 'a;

    /// The validating pass: reads every column of the group over `ctx`'s items, leaves `rd` after the group and
    /// returns the cursors at the column starts.
    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        level: Level,
    ) -> Result<Self::Reader, Error>;

    /// The next item of the subsequence from the column cursors, `restart` as the context had it.
    fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error>;

    /// An enum's (or `Option`'s) discriminant, `None` for every other type: a change of it restarts the fields
    /// declared after it.
    fn series_tag(&self) -> Option<u32> {
        None
    }

    /// An enum's tag run at its first item, from a fresh [skim](Self::skim) result; `None` for every other type.
    fn tag_run(_reader: &Self::Reader) -> Option<TagRun<'i>> {
        None
    }
}
