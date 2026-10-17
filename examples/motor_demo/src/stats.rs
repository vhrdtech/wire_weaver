//! What observing costs, counted on the host: per property how often it arrived, per level updates and message
//! bytes against the granted budget, missed sends, and what the same values would have taken as one `StreamData`
//! event per property.
//!
//! Bytes are whole serialized `ww_client_server::Event`s, the messages the link carries, rebuilt from what arrived:
//! a `PropertyUpdate` with the level's path, timestamp, missed count and the update's data. Framing (USB packets,
//! UDP datagrams) comes on top and is shared by every message in a frame, so it is left out of both sides.

use std::collections::BTreeMap;
use std::time::Instant;

use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrap, UNib32, UVlq32};
use wire_weaver_client::observe::Observed;
use ww_client_server::{Event, EventKind};
use ww_si::Second;
use ww_si::ww_numeric::NumericValue;

use crate::units::Human;

use crate::client::api_client::motor_demo_api_motor_demo::{
    motor_demo_api_control::ControlUpdate, motor_demo_api_foc::FocUpdate,
    motor_demo_api_telemetry::TelemetryUpdate,
};

/// The observable levels, by resource index at the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Foc = 0,
    Control = 1,
    Telemetry = 2,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Foc, Level::Control, Level::Telemetry];

    pub fn name(self) -> &'static str {
        match self {
            Level::Foc => "foc",
            Level::Control => "control",
            Level::Telemetry => "telemetry",
        }
    }

    /// The rate the controller computes the level's values at, Hz of simulated time.
    pub fn tier_hz(self) -> u32 {
        match self {
            Level::Foc => 25_000,
            Level::Control => 1_000,
            Level::Telemetry => 50,
        }
    }

    pub fn props(self) -> impl Iterator<Item = &'static Prop> {
        PROPS.iter().filter(move |p| p.level == self)
    }
}

/// An observable property.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub level: Level,
    /// Resource index in its level.
    pub index: u32,
    pub name: &'static str,
    /// What the host shows the value in: VESC's erpm, Ah and Wh where it has them, the base unit of the quantity
    /// otherwise.
    pub unit: &'static str,
}

impl Prop {
    /// `level.name`
    pub fn path(&self) -> String {
        format!("{}.{}", self.level.name(), self.name)
    }
}

/// A property present in an update.
#[derive(Clone, Debug)]
pub struct Present {
    pub index: u32,
    pub value: String,
    /// Bytes of the value alone.
    pub len: usize,
}

fn present<T: SerializeShrinkWrap + Human>(
    index: u32,
    unit: &str,
    value: &Option<T>,
    out: &mut Vec<Present>,
) {
    if let Some(v) = value {
        let mut buf = [0u8; 32];
        let len = v.to_ww_bytes(&mut buf).map(|b| b.len()).unwrap_or(0);
        out.push(Present {
            index,
            value: v.human(unit),
            len,
        });
    }
}

/// What an update carried: the properties present and the update's data as the device serialized it.
pub struct Carried {
    pub present: Vec<Present>,
    pub data: Vec<u8>,
}

/// The property table, and per level a function listing what an update carries. The data is serialized again with
/// the device's own `<Level>Update` type, the one its `poll_properties` writes, so it has the bytes the device sent.
macro_rules! levels {
    ($($level:ident $Update:ident $fn:ident { $($index:literal $name:ident $unit:literal;)* })*) => {
        /// Every observable property, in level and index order.
        pub const PROPS: &[Prop] = &[
            $($(Prop { level: Level::$level, index: $index, name: stringify!($name), unit: $unit },)*)*
        ];

        $(
            /// What an update of the level carried.
            pub fn $fn(u: &$Update) -> Carried {
                let mut present_ = vec![];
                $(present($index, $unit, &u.$name, &mut present_);)*
                let sent = crate::device::api_impl::$Update { $($name: u.$name,)* };
                let mut buf = [0u8; crate::device::MAX_MESSAGE_LEN];
                let data = sent.to_ww_bytes(&mut buf).map(|b| b.to_vec()).unwrap_or_default();
                Carried { present: present_, data }
            }
        )*
    };
}

levels! {
    Foc FocUpdate foc_present {
        0 i_a "A"; 1 i_b "A"; 2 i_c "A"; 3 i_alpha "A"; 4 i_beta "A"; 5 i_d "A"; 6 i_q "A";
        7 v_d "V"; 8 v_q "V"; 9 v_alpha "V"; 10 v_beta "V";
        11 duty_a ""; 12 duty_b ""; 13 duty_c "";
        14 phase "rad"; 15 phase_observer "rad"; 16 phase_encoder "rad";
        17 observer_x1 "Wb"; 18 observer_x2 "Wb"; 19 v_bus "V"; 20 phase_true "rad";
    }
    Control ControlUpdate control_present {
        0 mode ""; 1 sensor_mode ""; 2 duty_target ""; 3 current_target "A"; 4 speed_target "erpm";
        5 speed_electrical "erpm"; 6 speed "rpm"; 7 id_set "A"; 8 iq_set "A"; 9 id_filtered "A";
        10 iq_filtered "A"; 11 i_motor "A"; 12 i_input "A"; 13 duty_now ""; 14 torque "N·m"; 15 position "°";
        16 observer_error "°"; 17 speed_true "rpm";
    }
    Telemetry TelemetryUpdate telemetry_present {
        0 time "s"; 1 charge_drawn "Ah"; 2 charge_returned "Ah"; 3 energy_drawn "Wh"; 4 energy_returned "Wh";
        5 tachometer ""; 6 tachometer_abs ""; 7 v_in "V"; 8 power_in "W"; 9 avg_motor_current "A";
        10 avg_input_current "A"; 11 fault ""; 12 real_time_factor "";
    }
}

/// Find a property by `level.name` or by its name alone when that is unique.
pub fn find(name: &str) -> Option<&'static Prop> {
    if let Some((level, name)) = name.split_once('.') {
        return PROPS
            .iter()
            .find(|p| p.level.name() == level && p.name == name);
    }
    let mut found = PROPS.iter().filter(|p| p.name == name);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// Bytes of the `PropertyUpdate` event that carried `data`. The host unwraps the device's microseconds, on the wire
/// they are a wrapping `u32`.
pub fn property_update_len(level: Level, t: Option<&Second>, missed: u32, data: &[u8]) -> usize {
    let path = [UNib32(level as u32)];
    let t = t.map(|t| match (t.prefix, t_us(Some(t))) {
        (ww_si::Prefix::Micro, Some(us)) => {
            Second::from_parts(ww_si::Prefix::Micro, NumericValue::U32(us as u32))
        }
        _ => *t,
    });
    let event = Event {
        seq: UVlq32(0),
        result: Ok(EventKind::PropertyUpdate {
            path: RefVec::Slice { slice: &path },
            t,
            missed: (missed > 0).then_some(UNib32(missed)),
            data: TailBytes(data),
        }),
    };
    let mut buf = vec![0u8; data.len() + 64];
    event.to_ww_bytes(&mut buf).map(|b| b.len()).unwrap_or(0)
}

/// Bytes of a `StreamData` event carrying one property's value on its own path, as `stream_data_ser()` sends it.
pub fn stream_data_len(level: Level, index: u32, value_len: usize) -> usize {
    let path = [UNib32(level as u32), UNib32(index)];
    let data = vec![0u8; value_len];
    let event = Event {
        seq: UVlq32(0),
        result: Ok(EventKind::StreamData {
            path: RefVec::Slice { slice: &path },
            data: TailBytes(&data),
        }),
    };
    let mut buf = vec![0u8; value_len + 64];
    event.to_ww_bytes(&mut buf).map(|b| b.len()).unwrap_or(0)
}

/// Device time of an update, µs.
pub fn t_us(t: Option<&Second>) -> Option<u64> {
    match t?.value {
        NumericValue::U64(t) => Some(t),
        NumericValue::U32(t) => Some(t as u64),
        _ => None,
    }
}

/// Counts of one property.
#[derive(Clone, Debug, Default)]
pub struct PropStats {
    pub count: u64,
    pub last: String,
    /// Bytes as one `StreamData` per value.
    pub stream_data_bytes: u64,
}

/// Counts of one level.
#[derive(Clone, Debug, Default)]
pub struct LevelStats {
    pub updates: u64,
    pub bytes: u64,
    pub missed: u64,
    /// Device time of the first and the last update, µs.
    pub first_t: Option<u64>,
    pub last_t: Option<u64>,
    pub props: BTreeMap<u32, PropStats>,
}

impl LevelStats {
    pub fn stream_data_bytes(&self) -> u64 {
        self.props.values().map(|p| p.stream_data_bytes).sum()
    }

    /// Values received, all properties together.
    pub fn values(&self) -> u64 {
        self.props.values().map(|p| p.count).sum()
    }

    /// Simulated seconds between the first and the last update.
    pub fn sim_span_s(&self) -> f64 {
        match (self.first_t, self.last_t) {
            (Some(a), Some(b)) => (b - a) as f64 / 1e6,
            _ => 0.0,
        }
    }
}

/// Everything counted since [Stats::new] or the last [Stats::take].
#[derive(Clone, Debug)]
pub struct Stats {
    pub since: Instant,
    pub levels: BTreeMap<Level, LevelStats>,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    pub fn new() -> Self {
        Stats {
            since: Instant::now(),
            levels: BTreeMap::new(),
        }
    }

    /// Count one update of `level`: `data` is the update's data, `present` what it carried.
    pub fn record(
        &mut self,
        level: Level,
        t: Option<&Second>,
        missed: u32,
        data: &[u8],
        present: &[Present],
    ) {
        let l = self.levels.entry(level).or_default();
        l.updates += 1;
        l.bytes += property_update_len(level, t, missed, data) as u64;
        l.missed += missed as u64;
        if let Some(t) = t_us(t) {
            l.first_t.get_or_insert(t);
            l.last_t = Some(t);
        }
        for p in present {
            let s = l.props.entry(p.index).or_default();
            s.count += 1;
            s.last.clone_from(&p.value);
            s.stream_data_bytes += stream_data_len(level, p.index, p.len) as u64;
        }
    }

    /// Count an update as it arrived from an observer.
    pub fn record_observed<U>(
        &mut self,
        level: Level,
        o: &Observed<U>,
        carried: impl FnOnce(&U) -> Carried,
    ) {
        let c = carried(&o.value);
        self.record(level, o.t.as_ref(), o.missed, &c.data, &c.present);
    }

    /// The counts so far, starting over.
    pub fn take(&mut self) -> Stats {
        std::mem::take(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::shrink_wrap::DeserializeShrinkWrap;

    #[test]
    fn table_is_complete() {
        assert_eq!(PROPS.len(), 52);
        for level in Level::ALL {
            for (k, p) in level.props().enumerate() {
                assert_eq!(p.index, k as u32, "{}", p.path());
            }
        }
        assert_eq!(find("i_q").unwrap().path(), "foc.i_q");
        assert_eq!(find("control.speed_electrical").unwrap().index, 5);
        assert!(find("nope").is_none());
    }

    #[test]
    fn one_value_costs_more_alone() {
        // one current of the foc level, timestamp in µs
        let t = Second::from_parts(ww_si::Prefix::Micro, NumericValue::U64(123_456));
        let mut u = crate::device::api_impl::FocUpdate::from_ww_bytes(&[]).unwrap();
        u.i_q = Some(ww_si::Ampere::milli(1500i32));
        let mut buf = [0u8; 64];
        let data = u.to_ww_bytes(&mut buf).unwrap().to_vec();
        let update = property_update_len(Level::Foc, Some(&t), 0, &data);
        let alone = stream_data_len(Level::Foc, 6, 5);
        assert!(update > 5 && alone > 5, "{update} {alone}");
    }
}
