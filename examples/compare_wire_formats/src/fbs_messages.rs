pub use root::*;

const _: () = ::planus::check_version_compatibility("planus-1.3.0");

/// The root namespace
///
/// Generated from these locations:
/// * File `schema/messages.fbs`
#[no_implicit_prelude]
#[allow(clippy::needless_lifetimes)]
mod root {
    /// The namespace `wire_sizes`
    ///
    /// Generated from these locations:
    /// * File `schema/messages.fbs`
    pub mod wire_sizes {
        /// The struct `Vec3` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Struct `Vec3` in the file `schema/messages.fbs:9`
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Default,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct Vec3 {
            /// The field `x` in the struct `Vec3`
            pub x: f32,

            /// The field `y` in the struct `Vec3`
            pub y: f32,

            /// The field `z` in the struct `Vec3`
            pub z: f32,
        }

        /// # Safety
        /// The Planus compiler correctly calculates `ALIGNMENT` and `SIZE`.
        unsafe impl ::planus::Primitive for Vec3 {
            const ALIGNMENT: usize = 4;
            const SIZE: usize = 12;
        }

        #[allow(clippy::identity_op)]
        impl ::planus::WriteAsPrimitive<Vec3> for Vec3 {
            #[inline]
            fn write<const N: usize>(&self, cursor: ::planus::Cursor<'_, N>, buffer_position: u32) {
                let (cur, cursor) = cursor.split::<4, 8>();
                self.x.write(cur, buffer_position - 0);
                let (cur, cursor) = cursor.split::<4, 4>();
                self.y.write(cur, buffer_position - 4);
                let (cur, cursor) = cursor.split::<4, 0>();
                self.z.write(cur, buffer_position - 8);
                cursor.finish([]);
            }
        }

        impl ::planus::WriteAsOffset<Vec3> for Vec3 {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Vec3> {
                unsafe {
                    builder.write_with(12, 3, |buffer_position, bytes| {
                        let bytes = bytes.as_mut_ptr();

                        ::planus::WriteAsPrimitive::write(
                            self,
                            ::planus::Cursor::new(
                                &mut *(bytes as *mut [::core::mem::MaybeUninit<u8>; 12]),
                            ),
                            buffer_position,
                        );
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<Vec3> for Vec3 {
            type Prepared = Self;
            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self {
                *self
            }
        }

        impl ::planus::WriteAsOptional<Vec3> for Vec3 {
            type Prepared = Self;
            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> ::core::option::Option<Self> {
                ::core::option::Option::Some(*self)
            }
        }

        /// Reference to a deserialized [Vec3].
        #[derive(Copy, Clone)]
        pub struct Vec3Ref<'a>(::planus::ArrayWithStartOffset<'a, 12>);

        impl<'a> Vec3Ref<'a> {
            /// Getter for the [`x` field](Vec3#structfield.x).
            pub fn x(&self) -> f32 {
                let buffer = self.0.advance_as_array::<4>(0).unwrap();

                f32::from_le_bytes(*buffer.as_array())
            }

            /// Getter for the [`y` field](Vec3#structfield.y).
            pub fn y(&self) -> f32 {
                let buffer = self.0.advance_as_array::<4>(4).unwrap();

                f32::from_le_bytes(*buffer.as_array())
            }

            /// Getter for the [`z` field](Vec3#structfield.z).
            pub fn z(&self) -> f32 {
                let buffer = self.0.advance_as_array::<4>(8).unwrap();

                f32::from_le_bytes(*buffer.as_array())
            }
        }

        impl<'a> ::core::fmt::Debug for Vec3Ref<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("Vec3Ref");
                f.field("x", &self.x());
                f.field("y", &self.y());
                f.field("z", &self.z());
                f.finish()
            }
        }

        impl<'a> ::core::convert::From<::planus::ArrayWithStartOffset<'a, 12>> for Vec3Ref<'a> {
            fn from(array: ::planus::ArrayWithStartOffset<'a, 12>) -> Self {
                Self(array)
            }
        }

        impl<'a> ::core::convert::From<Vec3Ref<'a>> for Vec3 {
            #[allow(unreachable_code)]
            fn from(value: Vec3Ref<'a>) -> Self {
                Self {
                    x: value.x(),
                    y: value.y(),
                    z: value.z(),
                }
            }
        }

        impl<'a, 'b> ::core::cmp::PartialEq<Vec3Ref<'a>> for Vec3Ref<'b> {
            fn eq(&self, other: &Vec3Ref<'_>) -> bool {
                self.x() == other.x() && self.y() == other.y() && self.z() == other.z()
            }
        }

        impl<'a, 'b> ::core::cmp::PartialOrd<Vec3Ref<'a>> for Vec3Ref<'b> {
            fn partial_cmp(
                &self,
                other: &Vec3Ref<'_>,
            ) -> ::core::option::Option<::core::cmp::Ordering> {
                match self.x().partial_cmp(&other.x()) {
                    ::core::option::Option::Some(::core::cmp::Ordering::Equal) => (),
                    o => return o,
                }

                match self.y().partial_cmp(&other.y()) {
                    ::core::option::Option::Some(::core::cmp::Ordering::Equal) => (),
                    o => return o,
                }

                self.z().partial_cmp(&other.z())
            }
        }

        impl<'a> ::planus::TableRead<'a> for Vec3Ref<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                let buffer = buffer.advance_as_array::<12>(offset)?;
                ::core::result::Result::Ok(Self(buffer))
            }
        }

        impl<'a> ::planus::VectorRead<'a> for Vec3Ref<'a> {
            const STRIDE: usize = 12;

            #[inline]
            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> Self {
                Self(unsafe { buffer.unchecked_advance_as_array(offset) })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<Vec3> for Vec3 {
            const STRIDE: usize = 12;

            type Value = Vec3;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self::Value {
                *self
            }

            #[inline]
            unsafe fn write_values(
                values: &[Vec3],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 12];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (12 * i) as u32,
                    );
                }
            }
        }

        /// The table `Telemetry` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `Telemetry` in the file `schema/messages.fbs:15`
        #[derive(Clone, Debug, PartialEq, PartialOrd, ::serde::Serialize, ::serde::Deserialize)]
        pub struct Telemetry {
            /// The field `seq` in the table `Telemetry`
            pub seq: u16,
            /// The field `t_us` in the table `Telemetry`
            pub t_us: u64,
            /// The field `accel` in the table `Telemetry`
            pub accel: ::core::option::Option<self::Vec3>,
            /// The field `gyro` in the table `Telemetry`
            pub gyro: ::core::option::Option<self::Vec3>,
            /// The field `temp_c` in the table `Telemetry`
            pub temp_c: f32,
            /// The field `battery_mv` in the table `Telemetry`
            pub battery_mv: u16,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for Telemetry {
            fn default() -> Self {
                Self {
                    seq: 0,
                    t_us: 0,
                    accel: ::core::default::Default::default(),
                    gyro: ::core::default::Default::default(),
                    temp_c: 0.0,
                    battery_mv: 0,
                }
            }
        }

        impl Telemetry {
            /// Creates a [TelemetryBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> TelemetryBuilder<()> {
                TelemetryBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_seq: impl ::planus::WriteAsDefault<u16, u16>,
                field_t_us: impl ::planus::WriteAsDefault<u64, u64>,
                field_accel: impl ::planus::WriteAsOptional<self::Vec3>,
                field_gyro: impl ::planus::WriteAsOptional<self::Vec3>,
                field_temp_c: impl ::planus::WriteAsDefault<f32, f32>,
                field_battery_mv: impl ::planus::WriteAsDefault<u16, u16>,
            ) -> ::planus::Offset<Self> {
                let prepared_seq = field_seq.prepare(builder, &0);
                let prepared_t_us = field_t_us.prepare(builder, &0);
                let prepared_accel = field_accel.prepare(builder);
                let prepared_gyro = field_gyro.prepare(builder);
                let prepared_temp_c = field_temp_c.prepare(builder, &0.0);
                let prepared_battery_mv = field_battery_mv.prepare(builder, &0);

                let mut table_writer: ::planus::table_writer::TableWriter<16> =
                    ::core::default::Default::default();
                if prepared_t_us.is_some() {
                    table_writer.write_entry::<u64>(1);
                }
                if prepared_accel.is_some() {
                    table_writer.write_entry::<self::Vec3>(2);
                }
                if prepared_gyro.is_some() {
                    table_writer.write_entry::<self::Vec3>(3);
                }
                if prepared_temp_c.is_some() {
                    table_writer.write_entry::<f32>(4);
                }
                if prepared_seq.is_some() {
                    table_writer.write_entry::<u16>(0);
                }
                if prepared_battery_mv.is_some() {
                    table_writer.write_entry::<u16>(5);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_t_us) = prepared_t_us {
                            object_writer.write::<_, _, 8>(&prepared_t_us);
                        }
                        if let ::core::option::Option::Some(prepared_accel) = prepared_accel {
                            object_writer.write::<_, _, 12>(&prepared_accel);
                        }
                        if let ::core::option::Option::Some(prepared_gyro) = prepared_gyro {
                            object_writer.write::<_, _, 12>(&prepared_gyro);
                        }
                        if let ::core::option::Option::Some(prepared_temp_c) = prepared_temp_c {
                            object_writer.write::<_, _, 4>(&prepared_temp_c);
                        }
                        if let ::core::option::Option::Some(prepared_seq) = prepared_seq {
                            object_writer.write::<_, _, 2>(&prepared_seq);
                        }
                        if let ::core::option::Option::Some(prepared_battery_mv) =
                            prepared_battery_mv
                        {
                            object_writer.write::<_, _, 2>(&prepared_battery_mv);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<Telemetry>> for Telemetry {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Telemetry> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<Telemetry>> for Telemetry {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<Telemetry>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<Telemetry> for Telemetry {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Telemetry> {
                Telemetry::create(
                    builder,
                    self.seq,
                    self.t_us,
                    self.accel,
                    self.gyro,
                    self.temp_c,
                    self.battery_mv,
                )
            }
        }

        /// Builder for serializing an instance of the [Telemetry] type.
        ///
        /// Can be created using the [Telemetry::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct TelemetryBuilder<State>(State);

        impl TelemetryBuilder<()> {
            /// Setter for the [`seq` field](Telemetry#structfield.seq).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn seq<T0>(self, value: T0) -> TelemetryBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u16, u16>,
            {
                TelemetryBuilder((value,))
            }

            /// Sets the [`seq` field](Telemetry#structfield.seq) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn seq_as_default(self) -> TelemetryBuilder<(::planus::DefaultValue,)> {
                self.seq(::planus::DefaultValue)
            }
        }

        impl<T0> TelemetryBuilder<(T0,)> {
            /// Setter for the [`t_us` field](Telemetry#structfield.t_us).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_us<T1>(self, value: T1) -> TelemetryBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsDefault<u64, u64>,
            {
                let (v0,) = self.0;
                TelemetryBuilder((v0, value))
            }

            /// Sets the [`t_us` field](Telemetry#structfield.t_us) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_us_as_default(self) -> TelemetryBuilder<(T0, ::planus::DefaultValue)> {
                self.t_us(::planus::DefaultValue)
            }
        }

        impl<T0, T1> TelemetryBuilder<(T0, T1)> {
            /// Setter for the [`accel` field](Telemetry#structfield.accel).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn accel<T2>(self, value: T2) -> TelemetryBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsOptional<self::Vec3>,
            {
                let (v0, v1) = self.0;
                TelemetryBuilder((v0, v1, value))
            }

            /// Sets the [`accel` field](Telemetry#structfield.accel) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn accel_as_null(self) -> TelemetryBuilder<(T0, T1, ())> {
                self.accel(())
            }
        }

        impl<T0, T1, T2> TelemetryBuilder<(T0, T1, T2)> {
            /// Setter for the [`gyro` field](Telemetry#structfield.gyro).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn gyro<T3>(self, value: T3) -> TelemetryBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsOptional<self::Vec3>,
            {
                let (v0, v1, v2) = self.0;
                TelemetryBuilder((v0, v1, v2, value))
            }

            /// Sets the [`gyro` field](Telemetry#structfield.gyro) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn gyro_as_null(self) -> TelemetryBuilder<(T0, T1, T2, ())> {
                self.gyro(())
            }
        }

        impl<T0, T1, T2, T3> TelemetryBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`temp_c` field](Telemetry#structfield.temp_c).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn temp_c<T4>(self, value: T4) -> TelemetryBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsDefault<f32, f32>,
            {
                let (v0, v1, v2, v3) = self.0;
                TelemetryBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`temp_c` field](Telemetry#structfield.temp_c) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn temp_c_as_default(
                self,
            ) -> TelemetryBuilder<(T0, T1, T2, T3, ::planus::DefaultValue)> {
                self.temp_c(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4> TelemetryBuilder<(T0, T1, T2, T3, T4)> {
            /// Setter for the [`battery_mv` field](Telemetry#structfield.battery_mv).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn battery_mv<T5>(self, value: T5) -> TelemetryBuilder<(T0, T1, T2, T3, T4, T5)>
            where
                T5: ::planus::WriteAsDefault<u16, u16>,
            {
                let (v0, v1, v2, v3, v4) = self.0;
                TelemetryBuilder((v0, v1, v2, v3, v4, value))
            }

            /// Sets the [`battery_mv` field](Telemetry#structfield.battery_mv) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn battery_mv_as_default(
                self,
            ) -> TelemetryBuilder<(T0, T1, T2, T3, T4, ::planus::DefaultValue)> {
                self.battery_mv(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4, T5> TelemetryBuilder<(T0, T1, T2, T3, T4, T5)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [Telemetry].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<Telemetry>
            where
                Self: ::planus::WriteAsOffset<Telemetry>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsDefault<u64, u64>,
            T2: ::planus::WriteAsOptional<self::Vec3>,
            T3: ::planus::WriteAsOptional<self::Vec3>,
            T4: ::planus::WriteAsDefault<f32, f32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
        > ::planus::WriteAs<::planus::Offset<Telemetry>>
            for TelemetryBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            type Prepared = ::planus::Offset<Telemetry>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Telemetry> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsDefault<u64, u64>,
            T2: ::planus::WriteAsOptional<self::Vec3>,
            T3: ::planus::WriteAsOptional<self::Vec3>,
            T4: ::planus::WriteAsDefault<f32, f32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
        > ::planus::WriteAsOptional<::planus::Offset<Telemetry>>
            for TelemetryBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            type Prepared = ::planus::Offset<Telemetry>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<Telemetry>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsDefault<u64, u64>,
            T2: ::planus::WriteAsOptional<self::Vec3>,
            T3: ::planus::WriteAsOptional<self::Vec3>,
            T4: ::planus::WriteAsDefault<f32, f32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
        > ::planus::WriteAsOffset<Telemetry> for TelemetryBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Telemetry> {
                let (v0, v1, v2, v3, v4, v5) = &self.0;
                Telemetry::create(builder, v0, v1, v2, v3, v4, v5)
            }
        }

        /// Reference to a deserialized [Telemetry].
        #[derive(Copy, Clone)]
        pub struct TelemetryRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> TelemetryRef<'a> {
            /// Getter for the [`seq` field](Telemetry#structfield.seq).
            #[inline]
            pub fn seq(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(self.0.access(0, "Telemetry", "seq")?.unwrap_or(0))
            }

            /// Getter for the [`t_us` field](Telemetry#structfield.t_us).
            #[inline]
            pub fn t_us(&self) -> ::planus::Result<u64> {
                ::core::result::Result::Ok(self.0.access(1, "Telemetry", "t_us")?.unwrap_or(0))
            }

            /// Getter for the [`accel` field](Telemetry#structfield.accel).
            #[inline]
            pub fn accel(&self) -> ::planus::Result<::core::option::Option<self::Vec3Ref<'a>>> {
                self.0.access(2, "Telemetry", "accel")
            }

            /// Getter for the [`gyro` field](Telemetry#structfield.gyro).
            #[inline]
            pub fn gyro(&self) -> ::planus::Result<::core::option::Option<self::Vec3Ref<'a>>> {
                self.0.access(3, "Telemetry", "gyro")
            }

            /// Getter for the [`temp_c` field](Telemetry#structfield.temp_c).
            #[inline]
            pub fn temp_c(&self) -> ::planus::Result<f32> {
                ::core::result::Result::Ok(self.0.access(4, "Telemetry", "temp_c")?.unwrap_or(0.0))
            }

            /// Getter for the [`battery_mv` field](Telemetry#structfield.battery_mv).
            #[inline]
            pub fn battery_mv(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(
                    self.0.access(5, "Telemetry", "battery_mv")?.unwrap_or(0),
                )
            }
        }

        impl<'a> ::core::fmt::Debug for TelemetryRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("TelemetryRef");
                f.field("seq", &self.seq());
                f.field("t_us", &self.t_us());
                if let ::core::option::Option::Some(field_accel) = self.accel().transpose() {
                    f.field("accel", &field_accel);
                }
                if let ::core::option::Option::Some(field_gyro) = self.gyro().transpose() {
                    f.field("gyro", &field_gyro);
                }
                f.field("temp_c", &self.temp_c());
                f.field("battery_mv", &self.battery_mv());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<TelemetryRef<'a>> for Telemetry {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: TelemetryRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    seq: ::core::convert::TryInto::try_into(value.seq()?)?,
                    t_us: ::core::convert::TryInto::try_into(value.t_us()?)?,
                    accel: value.accel()?.map(::core::convert::Into::into),
                    gyro: value.gyro()?.map(::core::convert::Into::into),
                    temp_c: ::core::convert::TryInto::try_into(value.temp_c()?)?,
                    battery_mv: ::core::convert::TryInto::try_into(value.battery_mv()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for TelemetryRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for TelemetryRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[TelemetryRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<Telemetry>> for Telemetry {
            type Value = ::planus::Offset<Telemetry>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<Telemetry>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for TelemetryRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[TelemetryRef]", "read_as_root", 0)
                })
            }
        }

        /// The enum `LogLevel` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Enum `LogLevel` in the file `schema/messages.fbs:25`
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[repr(u8)]
        pub enum LogLevel {
            /// The variant `Trace` in the enum `LogLevel`
            Trace = 0,

            /// The variant `Debug` in the enum `LogLevel`
            Debug = 1,

            /// The variant `Info` in the enum `LogLevel`
            Info = 2,

            /// The variant `Warn` in the enum `LogLevel`
            Warn = 3,

            /// The variant `Error` in the enum `LogLevel`
            Error = 4,
        }

        impl LogLevel {
            /// Array containing all valid variants of LogLevel
            pub const ENUM_VALUES: [Self; 5] = [
                Self::Trace,
                Self::Debug,
                Self::Info,
                Self::Warn,
                Self::Error,
            ];
        }

        impl ::core::convert::TryFrom<u8> for LogLevel {
            type Error = ::planus::errors::UnknownEnumTagKind;
            #[inline]
            fn try_from(
                value: u8,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTagKind> {
                #[allow(clippy::match_single_binding)]
                match value {
                    0 => ::core::result::Result::Ok(LogLevel::Trace),
                    1 => ::core::result::Result::Ok(LogLevel::Debug),
                    2 => ::core::result::Result::Ok(LogLevel::Info),
                    3 => ::core::result::Result::Ok(LogLevel::Warn),
                    4 => ::core::result::Result::Ok(LogLevel::Error),

                    _ => ::core::result::Result::Err(::planus::errors::UnknownEnumTagKind {
                        tag: value as i128,
                    }),
                }
            }
        }

        impl ::core::convert::From<LogLevel> for u8 {
            #[inline]
            fn from(value: LogLevel) -> Self {
                value as u8
            }
        }

        /// # Safety
        /// The Planus compiler correctly calculates `ALIGNMENT` and `SIZE`.
        unsafe impl ::planus::Primitive for LogLevel {
            const ALIGNMENT: usize = 1;
            const SIZE: usize = 1;
        }

        impl ::planus::WriteAsPrimitive<LogLevel> for LogLevel {
            #[inline]
            fn write<const N: usize>(&self, cursor: ::planus::Cursor<'_, N>, buffer_position: u32) {
                (*self as u8).write(cursor, buffer_position);
            }
        }

        impl ::planus::WriteAs<LogLevel> for LogLevel {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> LogLevel {
                *self
            }
        }

        impl ::planus::WriteAsDefault<LogLevel, LogLevel> for LogLevel {
            type Prepared = Self;

            #[inline]
            fn prepare(
                &self,
                _builder: &mut ::planus::Builder,
                default: &LogLevel,
            ) -> ::core::option::Option<LogLevel> {
                if self == default {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(*self)
                }
            }
        }

        impl ::planus::WriteAsOptional<LogLevel> for LogLevel {
            type Prepared = Self;

            #[inline]
            fn prepare(
                &self,
                _builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<LogLevel> {
                ::core::option::Option::Some(*self)
            }
        }

        impl<'buf> ::planus::TableRead<'buf> for LogLevel {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                let n: u8 = ::planus::TableRead::from_buffer(buffer, offset)?;
                ::core::result::Result::Ok(::core::convert::TryInto::try_into(n)?)
            }
        }

        impl<'buf> ::planus::VectorReadInner<'buf> for LogLevel {
            type Error = ::planus::errors::UnknownEnumTag;
            const STRIDE: usize = 1;
            #[inline]
            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTag> {
                let value = unsafe { *buffer.buffer.get_unchecked(offset) };
                let value: ::core::result::Result<Self, _> =
                    ::core::convert::TryInto::try_into(value);
                value.map_err(|error_kind| {
                    error_kind.with_error_location(
                        "LogLevel",
                        "VectorRead::from_buffer",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<LogLevel> for LogLevel {
            const STRIDE: usize = 1;

            type Value = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self {
                *self
            }

            #[inline]
            unsafe fn write_values(
                values: &[Self],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 1];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - i as u32,
                    );
                }
            }
        }

        /// The table `LogRecord` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `LogRecord` in the file `schema/messages.fbs:27`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct LogRecord {
            /// The field `t_ms` in the table `LogRecord`
            pub t_ms: u64,
            /// The field `level` in the table `LogRecord`
            pub level: self::LogLevel,
            /// The field `target` in the table `LogRecord`
            pub target: ::core::option::Option<::planus::alloc::string::String>,
            /// The field `message` in the table `LogRecord`
            pub message: ::core::option::Option<::planus::alloc::string::String>,
            /// The field `line` in the table `LogRecord`
            pub line: u32,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for LogRecord {
            fn default() -> Self {
                Self {
                    t_ms: 0,
                    level: self::LogLevel::Trace,
                    target: ::core::default::Default::default(),
                    message: ::core::default::Default::default(),
                    line: 0,
                }
            }
        }

        impl LogRecord {
            /// Creates a [LogRecordBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> LogRecordBuilder<()> {
                LogRecordBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_t_ms: impl ::planus::WriteAsDefault<u64, u64>,
                field_level: impl ::planus::WriteAsDefault<self::LogLevel, self::LogLevel>,
                field_target: impl ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
                field_message: impl ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
                field_line: impl ::planus::WriteAsDefault<u32, u32>,
            ) -> ::planus::Offset<Self> {
                let prepared_t_ms = field_t_ms.prepare(builder, &0);
                let prepared_level = field_level.prepare(builder, &self::LogLevel::Trace);
                let prepared_target = field_target.prepare(builder);
                let prepared_message = field_message.prepare(builder);
                let prepared_line = field_line.prepare(builder, &0);

                let mut table_writer: ::planus::table_writer::TableWriter<14> =
                    ::core::default::Default::default();
                if prepared_t_ms.is_some() {
                    table_writer.write_entry::<u64>(0);
                }
                if prepared_target.is_some() {
                    table_writer.write_entry::<::planus::Offset<str>>(2);
                }
                if prepared_message.is_some() {
                    table_writer.write_entry::<::planus::Offset<str>>(3);
                }
                if prepared_line.is_some() {
                    table_writer.write_entry::<u32>(4);
                }
                if prepared_level.is_some() {
                    table_writer.write_entry::<self::LogLevel>(1);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_t_ms) = prepared_t_ms {
                            object_writer.write::<_, _, 8>(&prepared_t_ms);
                        }
                        if let ::core::option::Option::Some(prepared_target) = prepared_target {
                            object_writer.write::<_, _, 4>(&prepared_target);
                        }
                        if let ::core::option::Option::Some(prepared_message) = prepared_message {
                            object_writer.write::<_, _, 4>(&prepared_message);
                        }
                        if let ::core::option::Option::Some(prepared_line) = prepared_line {
                            object_writer.write::<_, _, 4>(&prepared_line);
                        }
                        if let ::core::option::Option::Some(prepared_level) = prepared_level {
                            object_writer.write::<_, _, 1>(&prepared_level);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<LogRecord>> for LogRecord {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LogRecord> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<LogRecord>> for LogRecord {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LogRecord>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<LogRecord> for LogRecord {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LogRecord> {
                LogRecord::create(
                    builder,
                    self.t_ms,
                    self.level,
                    &self.target,
                    &self.message,
                    self.line,
                )
            }
        }

        /// Builder for serializing an instance of the [LogRecord] type.
        ///
        /// Can be created using the [LogRecord::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct LogRecordBuilder<State>(State);

        impl LogRecordBuilder<()> {
            /// Setter for the [`t_ms` field](LogRecord#structfield.t_ms).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_ms<T0>(self, value: T0) -> LogRecordBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u64, u64>,
            {
                LogRecordBuilder((value,))
            }

            /// Sets the [`t_ms` field](LogRecord#structfield.t_ms) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_ms_as_default(self) -> LogRecordBuilder<(::planus::DefaultValue,)> {
                self.t_ms(::planus::DefaultValue)
            }
        }

        impl<T0> LogRecordBuilder<(T0,)> {
            /// Setter for the [`level` field](LogRecord#structfield.level).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn level<T1>(self, value: T1) -> LogRecordBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsDefault<self::LogLevel, self::LogLevel>,
            {
                let (v0,) = self.0;
                LogRecordBuilder((v0, value))
            }

            /// Sets the [`level` field](LogRecord#structfield.level) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn level_as_default(self) -> LogRecordBuilder<(T0, ::planus::DefaultValue)> {
                self.level(::planus::DefaultValue)
            }
        }

        impl<T0, T1> LogRecordBuilder<(T0, T1)> {
            /// Setter for the [`target` field](LogRecord#structfield.target).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn target<T2>(self, value: T2) -> LogRecordBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            {
                let (v0, v1) = self.0;
                LogRecordBuilder((v0, v1, value))
            }

            /// Sets the [`target` field](LogRecord#structfield.target) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn target_as_null(self) -> LogRecordBuilder<(T0, T1, ())> {
                self.target(())
            }
        }

        impl<T0, T1, T2> LogRecordBuilder<(T0, T1, T2)> {
            /// Setter for the [`message` field](LogRecord#structfield.message).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn message<T3>(self, value: T3) -> LogRecordBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            {
                let (v0, v1, v2) = self.0;
                LogRecordBuilder((v0, v1, v2, value))
            }

            /// Sets the [`message` field](LogRecord#structfield.message) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn message_as_null(self) -> LogRecordBuilder<(T0, T1, T2, ())> {
                self.message(())
            }
        }

        impl<T0, T1, T2, T3> LogRecordBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`line` field](LogRecord#structfield.line).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn line<T4>(self, value: T4) -> LogRecordBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsDefault<u32, u32>,
            {
                let (v0, v1, v2, v3) = self.0;
                LogRecordBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`line` field](LogRecord#structfield.line) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn line_as_default(
                self,
            ) -> LogRecordBuilder<(T0, T1, T2, T3, ::planus::DefaultValue)> {
                self.line(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4> LogRecordBuilder<(T0, T1, T2, T3, T4)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [LogRecord].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<LogRecord>
            where
                Self: ::planus::WriteAsOffset<LogRecord>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<self::LogLevel, self::LogLevel>,
            T2: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T3: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T4: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAs<::planus::Offset<LogRecord>>
            for LogRecordBuilder<(T0, T1, T2, T3, T4)>
        {
            type Prepared = ::planus::Offset<LogRecord>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LogRecord> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<self::LogLevel, self::LogLevel>,
            T2: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T3: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T4: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAsOptional<::planus::Offset<LogRecord>>
            for LogRecordBuilder<(T0, T1, T2, T3, T4)>
        {
            type Prepared = ::planus::Offset<LogRecord>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LogRecord>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<self::LogLevel, self::LogLevel>,
            T2: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T3: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T4: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAsOffset<LogRecord> for LogRecordBuilder<(T0, T1, T2, T3, T4)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LogRecord> {
                let (v0, v1, v2, v3, v4) = &self.0;
                LogRecord::create(builder, v0, v1, v2, v3, v4)
            }
        }

        /// Reference to a deserialized [LogRecord].
        #[derive(Copy, Clone)]
        pub struct LogRecordRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> LogRecordRef<'a> {
            /// Getter for the [`t_ms` field](LogRecord#structfield.t_ms).
            #[inline]
            pub fn t_ms(&self) -> ::planus::Result<u64> {
                ::core::result::Result::Ok(self.0.access(0, "LogRecord", "t_ms")?.unwrap_or(0))
            }

            /// Getter for the [`level` field](LogRecord#structfield.level).
            #[inline]
            pub fn level(&self) -> ::planus::Result<self::LogLevel> {
                ::core::result::Result::Ok(
                    self.0
                        .access(1, "LogRecord", "level")?
                        .unwrap_or(self::LogLevel::Trace),
                )
            }

            /// Getter for the [`target` field](LogRecord#structfield.target).
            #[inline]
            pub fn target(
                &self,
            ) -> ::planus::Result<::core::option::Option<&'a ::core::primitive::str>> {
                self.0.access(2, "LogRecord", "target")
            }

            /// Getter for the [`message` field](LogRecord#structfield.message).
            #[inline]
            pub fn message(
                &self,
            ) -> ::planus::Result<::core::option::Option<&'a ::core::primitive::str>> {
                self.0.access(3, "LogRecord", "message")
            }

            /// Getter for the [`line` field](LogRecord#structfield.line).
            #[inline]
            pub fn line(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(self.0.access(4, "LogRecord", "line")?.unwrap_or(0))
            }
        }

        impl<'a> ::core::fmt::Debug for LogRecordRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("LogRecordRef");
                f.field("t_ms", &self.t_ms());
                f.field("level", &self.level());
                if let ::core::option::Option::Some(field_target) = self.target().transpose() {
                    f.field("target", &field_target);
                }
                if let ::core::option::Option::Some(field_message) = self.message().transpose() {
                    f.field("message", &field_message);
                }
                f.field("line", &self.line());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<LogRecordRef<'a>> for LogRecord {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: LogRecordRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    t_ms: ::core::convert::TryInto::try_into(value.t_ms()?)?,
                    level: ::core::convert::TryInto::try_into(value.level()?)?,
                    target: value.target()?.map(::core::convert::Into::into),
                    message: value.message()?.map(::core::convert::Into::into),
                    line: ::core::convert::TryInto::try_into(value.line()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for LogRecordRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for LogRecordRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[LogRecordRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<LogRecord>> for LogRecord {
            type Value = ::planus::Offset<LogRecord>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<LogRecord>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for LogRecordRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[LogRecordRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `FirmwareChunk` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `FirmwareChunk` in the file `schema/messages.fbs:36`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct FirmwareChunk {
            /// The field `offset` in the table `FirmwareChunk`
            pub offset: u32,
            /// The field `total` in the table `FirmwareChunk`
            pub total: u32,
            /// The field `crc32` in the table `FirmwareChunk`
            pub crc32: u32,
            /// The field `last` in the table `FirmwareChunk`
            pub last: bool,
            /// The field `data` in the table `FirmwareChunk`
            pub data: ::core::option::Option<::planus::alloc::vec::Vec<u8>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for FirmwareChunk {
            fn default() -> Self {
                Self {
                    offset: 0,
                    total: 0,
                    crc32: 0,
                    last: false,
                    data: ::core::default::Default::default(),
                }
            }
        }

        impl FirmwareChunk {
            /// Creates a [FirmwareChunkBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> FirmwareChunkBuilder<()> {
                FirmwareChunkBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_offset: impl ::planus::WriteAsDefault<u32, u32>,
                field_total: impl ::planus::WriteAsDefault<u32, u32>,
                field_crc32: impl ::planus::WriteAsDefault<u32, u32>,
                field_last: impl ::planus::WriteAsDefault<bool, bool>,
                field_data: impl ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            ) -> ::planus::Offset<Self> {
                let prepared_offset = field_offset.prepare(builder, &0);
                let prepared_total = field_total.prepare(builder, &0);
                let prepared_crc32 = field_crc32.prepare(builder, &0);
                let prepared_last = field_last.prepare(builder, &false);
                let prepared_data = field_data.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<14> =
                    ::core::default::Default::default();
                if prepared_offset.is_some() {
                    table_writer.write_entry::<u32>(0);
                }
                if prepared_total.is_some() {
                    table_writer.write_entry::<u32>(1);
                }
                if prepared_crc32.is_some() {
                    table_writer.write_entry::<u32>(2);
                }
                if prepared_data.is_some() {
                    table_writer.write_entry::<::planus::Offset<[u8]>>(4);
                }
                if prepared_last.is_some() {
                    table_writer.write_entry::<bool>(3);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_offset) = prepared_offset {
                            object_writer.write::<_, _, 4>(&prepared_offset);
                        }
                        if let ::core::option::Option::Some(prepared_total) = prepared_total {
                            object_writer.write::<_, _, 4>(&prepared_total);
                        }
                        if let ::core::option::Option::Some(prepared_crc32) = prepared_crc32 {
                            object_writer.write::<_, _, 4>(&prepared_crc32);
                        }
                        if let ::core::option::Option::Some(prepared_data) = prepared_data {
                            object_writer.write::<_, _, 4>(&prepared_data);
                        }
                        if let ::core::option::Option::Some(prepared_last) = prepared_last {
                            object_writer.write::<_, _, 1>(&prepared_last);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<FirmwareChunk>> for FirmwareChunk {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<FirmwareChunk> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<FirmwareChunk>> for FirmwareChunk {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<FirmwareChunk>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<FirmwareChunk> for FirmwareChunk {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<FirmwareChunk> {
                FirmwareChunk::create(
                    builder,
                    self.offset,
                    self.total,
                    self.crc32,
                    self.last,
                    &self.data,
                )
            }
        }

        /// Builder for serializing an instance of the [FirmwareChunk] type.
        ///
        /// Can be created using the [FirmwareChunk::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct FirmwareChunkBuilder<State>(State);

        impl FirmwareChunkBuilder<()> {
            /// Setter for the [`offset` field](FirmwareChunk#structfield.offset).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn offset<T0>(self, value: T0) -> FirmwareChunkBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u32, u32>,
            {
                FirmwareChunkBuilder((value,))
            }

            /// Sets the [`offset` field](FirmwareChunk#structfield.offset) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn offset_as_default(self) -> FirmwareChunkBuilder<(::planus::DefaultValue,)> {
                self.offset(::planus::DefaultValue)
            }
        }

        impl<T0> FirmwareChunkBuilder<(T0,)> {
            /// Setter for the [`total` field](FirmwareChunk#structfield.total).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn total<T1>(self, value: T1) -> FirmwareChunkBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsDefault<u32, u32>,
            {
                let (v0,) = self.0;
                FirmwareChunkBuilder((v0, value))
            }

            /// Sets the [`total` field](FirmwareChunk#structfield.total) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn total_as_default(self) -> FirmwareChunkBuilder<(T0, ::planus::DefaultValue)> {
                self.total(::planus::DefaultValue)
            }
        }

        impl<T0, T1> FirmwareChunkBuilder<(T0, T1)> {
            /// Setter for the [`crc32` field](FirmwareChunk#structfield.crc32).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn crc32<T2>(self, value: T2) -> FirmwareChunkBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsDefault<u32, u32>,
            {
                let (v0, v1) = self.0;
                FirmwareChunkBuilder((v0, v1, value))
            }

            /// Sets the [`crc32` field](FirmwareChunk#structfield.crc32) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn crc32_as_default(
                self,
            ) -> FirmwareChunkBuilder<(T0, T1, ::planus::DefaultValue)> {
                self.crc32(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2> FirmwareChunkBuilder<(T0, T1, T2)> {
            /// Setter for the [`last` field](FirmwareChunk#structfield.last).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn last<T3>(self, value: T3) -> FirmwareChunkBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsDefault<bool, bool>,
            {
                let (v0, v1, v2) = self.0;
                FirmwareChunkBuilder((v0, v1, v2, value))
            }

            /// Sets the [`last` field](FirmwareChunk#structfield.last) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn last_as_default(
                self,
            ) -> FirmwareChunkBuilder<(T0, T1, T2, ::planus::DefaultValue)> {
                self.last(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3> FirmwareChunkBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`data` field](FirmwareChunk#structfield.data).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn data<T4>(self, value: T4) -> FirmwareChunkBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            {
                let (v0, v1, v2, v3) = self.0;
                FirmwareChunkBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`data` field](FirmwareChunk#structfield.data) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn data_as_null(self) -> FirmwareChunkBuilder<(T0, T1, T2, T3, ())> {
                self.data(())
            }
        }

        impl<T0, T1, T2, T3, T4> FirmwareChunkBuilder<(T0, T1, T2, T3, T4)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [FirmwareChunk].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<FirmwareChunk>
            where
                Self: ::planus::WriteAsOffset<FirmwareChunk>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<u32, u32>,
            T2: ::planus::WriteAsDefault<u32, u32>,
            T3: ::planus::WriteAsDefault<bool, bool>,
            T4: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAs<::planus::Offset<FirmwareChunk>>
            for FirmwareChunkBuilder<(T0, T1, T2, T3, T4)>
        {
            type Prepared = ::planus::Offset<FirmwareChunk>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<FirmwareChunk> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<u32, u32>,
            T2: ::planus::WriteAsDefault<u32, u32>,
            T3: ::planus::WriteAsDefault<bool, bool>,
            T4: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAsOptional<::planus::Offset<FirmwareChunk>>
            for FirmwareChunkBuilder<(T0, T1, T2, T3, T4)>
        {
            type Prepared = ::planus::Offset<FirmwareChunk>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<FirmwareChunk>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<u32, u32>,
            T2: ::planus::WriteAsDefault<u32, u32>,
            T3: ::planus::WriteAsDefault<bool, bool>,
            T4: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAsOffset<FirmwareChunk> for FirmwareChunkBuilder<(T0, T1, T2, T3, T4)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<FirmwareChunk> {
                let (v0, v1, v2, v3, v4) = &self.0;
                FirmwareChunk::create(builder, v0, v1, v2, v3, v4)
            }
        }

        /// Reference to a deserialized [FirmwareChunk].
        #[derive(Copy, Clone)]
        pub struct FirmwareChunkRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> FirmwareChunkRef<'a> {
            /// Getter for the [`offset` field](FirmwareChunk#structfield.offset).
            #[inline]
            pub fn offset(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(
                    self.0.access(0, "FirmwareChunk", "offset")?.unwrap_or(0),
                )
            }

            /// Getter for the [`total` field](FirmwareChunk#structfield.total).
            #[inline]
            pub fn total(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(self.0.access(1, "FirmwareChunk", "total")?.unwrap_or(0))
            }

            /// Getter for the [`crc32` field](FirmwareChunk#structfield.crc32).
            #[inline]
            pub fn crc32(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(self.0.access(2, "FirmwareChunk", "crc32")?.unwrap_or(0))
            }

            /// Getter for the [`last` field](FirmwareChunk#structfield.last).
            #[inline]
            pub fn last(&self) -> ::planus::Result<bool> {
                ::core::result::Result::Ok(
                    self.0.access(3, "FirmwareChunk", "last")?.unwrap_or(false),
                )
            }

            /// Getter for the [`data` field](FirmwareChunk#structfield.data).
            #[inline]
            pub fn data(&self) -> ::planus::Result<::core::option::Option<&'a [u8]>> {
                self.0.access(4, "FirmwareChunk", "data")
            }
        }

        impl<'a> ::core::fmt::Debug for FirmwareChunkRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("FirmwareChunkRef");
                f.field("offset", &self.offset());
                f.field("total", &self.total());
                f.field("crc32", &self.crc32());
                f.field("last", &self.last());
                if let ::core::option::Option::Some(field_data) = self.data().transpose() {
                    f.field("data", &field_data);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<FirmwareChunkRef<'a>> for FirmwareChunk {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: FirmwareChunkRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    offset: ::core::convert::TryInto::try_into(value.offset()?)?,
                    total: ::core::convert::TryInto::try_into(value.total()?)?,
                    crc32: ::core::convert::TryInto::try_into(value.crc32()?)?,
                    last: ::core::convert::TryInto::try_into(value.last()?)?,
                    data: value.data()?.map(|v| v.to_vec()),
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for FirmwareChunkRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for FirmwareChunkRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[FirmwareChunkRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<FirmwareChunk>> for FirmwareChunk {
            type Value = ::planus::Offset<FirmwareChunk>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<FirmwareChunk>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for FirmwareChunkRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[FirmwareChunkRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `DeviceConfig` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `DeviceConfig` in the file `schema/messages.fbs:45`
        #[derive(Clone, Debug, PartialEq, PartialOrd, ::serde::Serialize, ::serde::Deserialize)]
        pub struct DeviceConfig {
            /// The field `name` in the table `DeviceConfig`
            pub name: ::core::option::Option<::planus::alloc::string::String>,
            /// The field `baud` in the table `DeviceConfig`
            pub baud: ::core::option::Option<u32>,
            /// The field `address` in the table `DeviceConfig`
            pub address: ::core::option::Option<u8>,
            /// The field `retries` in the table `DeviceConfig`
            pub retries: ::core::option::Option<u8>,
            /// The field `timeout_ms` in the table `DeviceConfig`
            pub timeout_ms: ::core::option::Option<u16>,
            /// The field `enabled` in the table `DeviceConfig`
            pub enabled: ::core::option::Option<bool>,
            /// The field `gain` in the table `DeviceConfig`
            pub gain: ::core::option::Option<f32>,
            /// The field `label` in the table `DeviceConfig`
            pub label: ::core::option::Option<::planus::alloc::string::String>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for DeviceConfig {
            fn default() -> Self {
                Self {
                    name: ::core::default::Default::default(),
                    baud: ::core::default::Default::default(),
                    address: ::core::default::Default::default(),
                    retries: ::core::default::Default::default(),
                    timeout_ms: ::core::default::Default::default(),
                    enabled: ::core::default::Default::default(),
                    gain: ::core::default::Default::default(),
                    label: ::core::default::Default::default(),
                }
            }
        }

        impl DeviceConfig {
            /// Creates a [DeviceConfigBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> DeviceConfigBuilder<()> {
                DeviceConfigBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_name: impl ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
                field_baud: impl ::planus::WriteAsOptional<u32>,
                field_address: impl ::planus::WriteAsOptional<u8>,
                field_retries: impl ::planus::WriteAsOptional<u8>,
                field_timeout_ms: impl ::planus::WriteAsOptional<u16>,
                field_enabled: impl ::planus::WriteAsOptional<bool>,
                field_gain: impl ::planus::WriteAsOptional<f32>,
                field_label: impl ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            ) -> ::planus::Offset<Self> {
                let prepared_name = field_name.prepare(builder);
                let prepared_baud = field_baud.prepare(builder);
                let prepared_address = field_address.prepare(builder);
                let prepared_retries = field_retries.prepare(builder);
                let prepared_timeout_ms = field_timeout_ms.prepare(builder);
                let prepared_enabled = field_enabled.prepare(builder);
                let prepared_gain = field_gain.prepare(builder);
                let prepared_label = field_label.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<20> =
                    ::core::default::Default::default();
                if prepared_name.is_some() {
                    table_writer.write_entry::<::planus::Offset<str>>(0);
                }
                if prepared_baud.is_some() {
                    table_writer.write_entry::<u32>(1);
                }
                if prepared_gain.is_some() {
                    table_writer.write_entry::<f32>(6);
                }
                if prepared_label.is_some() {
                    table_writer.write_entry::<::planus::Offset<str>>(7);
                }
                if prepared_timeout_ms.is_some() {
                    table_writer.write_entry::<u16>(4);
                }
                if prepared_address.is_some() {
                    table_writer.write_entry::<u8>(2);
                }
                if prepared_retries.is_some() {
                    table_writer.write_entry::<u8>(3);
                }
                if prepared_enabled.is_some() {
                    table_writer.write_entry::<bool>(5);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_name) = prepared_name {
                            object_writer.write::<_, _, 4>(&prepared_name);
                        }
                        if let ::core::option::Option::Some(prepared_baud) = prepared_baud {
                            object_writer.write::<_, _, 4>(&prepared_baud);
                        }
                        if let ::core::option::Option::Some(prepared_gain) = prepared_gain {
                            object_writer.write::<_, _, 4>(&prepared_gain);
                        }
                        if let ::core::option::Option::Some(prepared_label) = prepared_label {
                            object_writer.write::<_, _, 4>(&prepared_label);
                        }
                        if let ::core::option::Option::Some(prepared_timeout_ms) =
                            prepared_timeout_ms
                        {
                            object_writer.write::<_, _, 2>(&prepared_timeout_ms);
                        }
                        if let ::core::option::Option::Some(prepared_address) = prepared_address {
                            object_writer.write::<_, _, 1>(&prepared_address);
                        }
                        if let ::core::option::Option::Some(prepared_retries) = prepared_retries {
                            object_writer.write::<_, _, 1>(&prepared_retries);
                        }
                        if let ::core::option::Option::Some(prepared_enabled) = prepared_enabled {
                            object_writer.write::<_, _, 1>(&prepared_enabled);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<DeviceConfig>> for DeviceConfig {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<DeviceConfig> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<DeviceConfig>> for DeviceConfig {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<DeviceConfig>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<DeviceConfig> for DeviceConfig {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<DeviceConfig> {
                DeviceConfig::create(
                    builder,
                    &self.name,
                    self.baud,
                    self.address,
                    self.retries,
                    self.timeout_ms,
                    self.enabled,
                    self.gain,
                    &self.label,
                )
            }
        }

        /// Builder for serializing an instance of the [DeviceConfig] type.
        ///
        /// Can be created using the [DeviceConfig::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct DeviceConfigBuilder<State>(State);

        impl DeviceConfigBuilder<()> {
            /// Setter for the [`name` field](DeviceConfig#structfield.name).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn name<T0>(self, value: T0) -> DeviceConfigBuilder<(T0,)>
            where
                T0: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            {
                DeviceConfigBuilder((value,))
            }

            /// Sets the [`name` field](DeviceConfig#structfield.name) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn name_as_null(self) -> DeviceConfigBuilder<((),)> {
                self.name(())
            }
        }

        impl<T0> DeviceConfigBuilder<(T0,)> {
            /// Setter for the [`baud` field](DeviceConfig#structfield.baud).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn baud<T1>(self, value: T1) -> DeviceConfigBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsOptional<u32>,
            {
                let (v0,) = self.0;
                DeviceConfigBuilder((v0, value))
            }

            /// Sets the [`baud` field](DeviceConfig#structfield.baud) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn baud_as_null(self) -> DeviceConfigBuilder<(T0, ())> {
                self.baud(())
            }
        }

        impl<T0, T1> DeviceConfigBuilder<(T0, T1)> {
            /// Setter for the [`address` field](DeviceConfig#structfield.address).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn address<T2>(self, value: T2) -> DeviceConfigBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsOptional<u8>,
            {
                let (v0, v1) = self.0;
                DeviceConfigBuilder((v0, v1, value))
            }

            /// Sets the [`address` field](DeviceConfig#structfield.address) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn address_as_null(self) -> DeviceConfigBuilder<(T0, T1, ())> {
                self.address(())
            }
        }

        impl<T0, T1, T2> DeviceConfigBuilder<(T0, T1, T2)> {
            /// Setter for the [`retries` field](DeviceConfig#structfield.retries).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn retries<T3>(self, value: T3) -> DeviceConfigBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsOptional<u8>,
            {
                let (v0, v1, v2) = self.0;
                DeviceConfigBuilder((v0, v1, v2, value))
            }

            /// Sets the [`retries` field](DeviceConfig#structfield.retries) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn retries_as_null(self) -> DeviceConfigBuilder<(T0, T1, T2, ())> {
                self.retries(())
            }
        }

        impl<T0, T1, T2, T3> DeviceConfigBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`timeout_ms` field](DeviceConfig#structfield.timeout_ms).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn timeout_ms<T4>(self, value: T4) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsOptional<u16>,
            {
                let (v0, v1, v2, v3) = self.0;
                DeviceConfigBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`timeout_ms` field](DeviceConfig#structfield.timeout_ms) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn timeout_ms_as_null(self) -> DeviceConfigBuilder<(T0, T1, T2, T3, ())> {
                self.timeout_ms(())
            }
        }

        impl<T0, T1, T2, T3, T4> DeviceConfigBuilder<(T0, T1, T2, T3, T4)> {
            /// Setter for the [`enabled` field](DeviceConfig#structfield.enabled).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn enabled<T5>(self, value: T5) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5)>
            where
                T5: ::planus::WriteAsOptional<bool>,
            {
                let (v0, v1, v2, v3, v4) = self.0;
                DeviceConfigBuilder((v0, v1, v2, v3, v4, value))
            }

            /// Sets the [`enabled` field](DeviceConfig#structfield.enabled) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn enabled_as_null(self) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, ())> {
                self.enabled(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5)> {
            /// Setter for the [`gain` field](DeviceConfig#structfield.gain).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn gain<T6>(self, value: T6) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6)>
            where
                T6: ::planus::WriteAsOptional<f32>,
            {
                let (v0, v1, v2, v3, v4, v5) = self.0;
                DeviceConfigBuilder((v0, v1, v2, v3, v4, v5, value))
            }

            /// Sets the [`gain` field](DeviceConfig#structfield.gain) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn gain_as_null(self) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, ())> {
                self.gain(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6)> {
            /// Setter for the [`label` field](DeviceConfig#structfield.label).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn label<T7>(
                self,
                value: T7,
            ) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
            where
                T7: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            {
                let (v0, v1, v2, v3, v4, v5, v6) = self.0;
                DeviceConfigBuilder((v0, v1, v2, v3, v4, v5, v6, value))
            }

            /// Sets the [`label` field](DeviceConfig#structfield.label) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn label_as_null(self) -> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, ())> {
                self.label(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6, T7> DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [DeviceConfig].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<DeviceConfig>
            where
                Self: ::planus::WriteAsOffset<DeviceConfig>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T1: ::planus::WriteAsOptional<u32>,
            T2: ::planus::WriteAsOptional<u8>,
            T3: ::planus::WriteAsOptional<u8>,
            T4: ::planus::WriteAsOptional<u16>,
            T5: ::planus::WriteAsOptional<bool>,
            T6: ::planus::WriteAsOptional<f32>,
            T7: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
        > ::planus::WriteAs<::planus::Offset<DeviceConfig>>
            for DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<DeviceConfig>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<DeviceConfig> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T1: ::planus::WriteAsOptional<u32>,
            T2: ::planus::WriteAsOptional<u8>,
            T3: ::planus::WriteAsOptional<u8>,
            T4: ::planus::WriteAsOptional<u16>,
            T5: ::planus::WriteAsOptional<bool>,
            T6: ::planus::WriteAsOptional<f32>,
            T7: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
        > ::planus::WriteAsOptional<::planus::Offset<DeviceConfig>>
            for DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<DeviceConfig>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<DeviceConfig>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
            T1: ::planus::WriteAsOptional<u32>,
            T2: ::planus::WriteAsOptional<u8>,
            T3: ::planus::WriteAsOptional<u8>,
            T4: ::planus::WriteAsOptional<u16>,
            T5: ::planus::WriteAsOptional<bool>,
            T6: ::planus::WriteAsOptional<f32>,
            T7: ::planus::WriteAsOptional<::planus::Offset<::core::primitive::str>>,
        > ::planus::WriteAsOffset<DeviceConfig>
            for DeviceConfigBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<DeviceConfig> {
                let (v0, v1, v2, v3, v4, v5, v6, v7) = &self.0;
                DeviceConfig::create(builder, v0, v1, v2, v3, v4, v5, v6, v7)
            }
        }

        /// Reference to a deserialized [DeviceConfig].
        #[derive(Copy, Clone)]
        pub struct DeviceConfigRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> DeviceConfigRef<'a> {
            /// Getter for the [`name` field](DeviceConfig#structfield.name).
            #[inline]
            pub fn name(
                &self,
            ) -> ::planus::Result<::core::option::Option<&'a ::core::primitive::str>> {
                self.0.access(0, "DeviceConfig", "name")
            }

            /// Getter for the [`baud` field](DeviceConfig#structfield.baud).
            #[inline]
            pub fn baud(&self) -> ::planus::Result<::core::option::Option<u32>> {
                self.0.access(1, "DeviceConfig", "baud")
            }

            /// Getter for the [`address` field](DeviceConfig#structfield.address).
            #[inline]
            pub fn address(&self) -> ::planus::Result<::core::option::Option<u8>> {
                self.0.access(2, "DeviceConfig", "address")
            }

            /// Getter for the [`retries` field](DeviceConfig#structfield.retries).
            #[inline]
            pub fn retries(&self) -> ::planus::Result<::core::option::Option<u8>> {
                self.0.access(3, "DeviceConfig", "retries")
            }

            /// Getter for the [`timeout_ms` field](DeviceConfig#structfield.timeout_ms).
            #[inline]
            pub fn timeout_ms(&self) -> ::planus::Result<::core::option::Option<u16>> {
                self.0.access(4, "DeviceConfig", "timeout_ms")
            }

            /// Getter for the [`enabled` field](DeviceConfig#structfield.enabled).
            #[inline]
            pub fn enabled(&self) -> ::planus::Result<::core::option::Option<bool>> {
                self.0.access(5, "DeviceConfig", "enabled")
            }

            /// Getter for the [`gain` field](DeviceConfig#structfield.gain).
            #[inline]
            pub fn gain(&self) -> ::planus::Result<::core::option::Option<f32>> {
                self.0.access(6, "DeviceConfig", "gain")
            }

            /// Getter for the [`label` field](DeviceConfig#structfield.label).
            #[inline]
            pub fn label(
                &self,
            ) -> ::planus::Result<::core::option::Option<&'a ::core::primitive::str>> {
                self.0.access(7, "DeviceConfig", "label")
            }
        }

        impl<'a> ::core::fmt::Debug for DeviceConfigRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("DeviceConfigRef");
                if let ::core::option::Option::Some(field_name) = self.name().transpose() {
                    f.field("name", &field_name);
                }
                if let ::core::option::Option::Some(field_baud) = self.baud().transpose() {
                    f.field("baud", &field_baud);
                }
                if let ::core::option::Option::Some(field_address) = self.address().transpose() {
                    f.field("address", &field_address);
                }
                if let ::core::option::Option::Some(field_retries) = self.retries().transpose() {
                    f.field("retries", &field_retries);
                }
                if let ::core::option::Option::Some(field_timeout_ms) =
                    self.timeout_ms().transpose()
                {
                    f.field("timeout_ms", &field_timeout_ms);
                }
                if let ::core::option::Option::Some(field_enabled) = self.enabled().transpose() {
                    f.field("enabled", &field_enabled);
                }
                if let ::core::option::Option::Some(field_gain) = self.gain().transpose() {
                    f.field("gain", &field_gain);
                }
                if let ::core::option::Option::Some(field_label) = self.label().transpose() {
                    f.field("label", &field_label);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<DeviceConfigRef<'a>> for DeviceConfig {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: DeviceConfigRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    name: value.name()?.map(::core::convert::Into::into),
                    baud: if let ::core::option::Option::Some(baud) = value.baud()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(baud)?)
                    } else {
                        ::core::option::Option::None
                    },
                    address: if let ::core::option::Option::Some(address) = value.address()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(address)?)
                    } else {
                        ::core::option::Option::None
                    },
                    retries: if let ::core::option::Option::Some(retries) = value.retries()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(retries)?)
                    } else {
                        ::core::option::Option::None
                    },
                    timeout_ms: if let ::core::option::Option::Some(timeout_ms) =
                        value.timeout_ms()?
                    {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(
                            timeout_ms,
                        )?)
                    } else {
                        ::core::option::Option::None
                    },
                    enabled: if let ::core::option::Option::Some(enabled) = value.enabled()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(enabled)?)
                    } else {
                        ::core::option::Option::None
                    },
                    gain: if let ::core::option::Option::Some(gain) = value.gain()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(gain)?)
                    } else {
                        ::core::option::Option::None
                    },
                    label: value.label()?.map(::core::convert::Into::into),
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for DeviceConfigRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for DeviceConfigRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[DeviceConfigRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<DeviceConfig>> for DeviceConfig {
            type Value = ::planus::Offset<DeviceConfig>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<DeviceConfig>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for DeviceConfigRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[DeviceConfigRef]", "read_as_root", 0)
                })
            }
        }

        /// The enum `FixKind` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Enum `FixKind` in the file `schema/messages.fbs:57`
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[repr(u8)]
        pub enum FixKind {
            /// The variant `NoFix` in the enum `FixKind`
            NoFix = 0,

            /// The variant `Fix2d` in the enum `FixKind`
            Fix2d = 1,

            /// The variant `Fix3d` in the enum `FixKind`
            Fix3d = 2,

            /// The variant `Dgps` in the enum `FixKind`
            Dgps = 3,
        }

        impl FixKind {
            /// Array containing all valid variants of FixKind
            pub const ENUM_VALUES: [Self; 4] = [Self::NoFix, Self::Fix2d, Self::Fix3d, Self::Dgps];
        }

        impl ::core::convert::TryFrom<u8> for FixKind {
            type Error = ::planus::errors::UnknownEnumTagKind;
            #[inline]
            fn try_from(
                value: u8,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTagKind> {
                #[allow(clippy::match_single_binding)]
                match value {
                    0 => ::core::result::Result::Ok(FixKind::NoFix),
                    1 => ::core::result::Result::Ok(FixKind::Fix2d),
                    2 => ::core::result::Result::Ok(FixKind::Fix3d),
                    3 => ::core::result::Result::Ok(FixKind::Dgps),

                    _ => ::core::result::Result::Err(::planus::errors::UnknownEnumTagKind {
                        tag: value as i128,
                    }),
                }
            }
        }

        impl ::core::convert::From<FixKind> for u8 {
            #[inline]
            fn from(value: FixKind) -> Self {
                value as u8
            }
        }

        /// # Safety
        /// The Planus compiler correctly calculates `ALIGNMENT` and `SIZE`.
        unsafe impl ::planus::Primitive for FixKind {
            const ALIGNMENT: usize = 1;
            const SIZE: usize = 1;
        }

        impl ::planus::WriteAsPrimitive<FixKind> for FixKind {
            #[inline]
            fn write<const N: usize>(&self, cursor: ::planus::Cursor<'_, N>, buffer_position: u32) {
                (*self as u8).write(cursor, buffer_position);
            }
        }

        impl ::planus::WriteAs<FixKind> for FixKind {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> FixKind {
                *self
            }
        }

        impl ::planus::WriteAsDefault<FixKind, FixKind> for FixKind {
            type Prepared = Self;

            #[inline]
            fn prepare(
                &self,
                _builder: &mut ::planus::Builder,
                default: &FixKind,
            ) -> ::core::option::Option<FixKind> {
                if self == default {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(*self)
                }
            }
        }

        impl ::planus::WriteAsOptional<FixKind> for FixKind {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> ::core::option::Option<FixKind> {
                ::core::option::Option::Some(*self)
            }
        }

        impl<'buf> ::planus::TableRead<'buf> for FixKind {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                let n: u8 = ::planus::TableRead::from_buffer(buffer, offset)?;
                ::core::result::Result::Ok(::core::convert::TryInto::try_into(n)?)
            }
        }

        impl<'buf> ::planus::VectorReadInner<'buf> for FixKind {
            type Error = ::planus::errors::UnknownEnumTag;
            const STRIDE: usize = 1;
            #[inline]
            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTag> {
                let value = unsafe { *buffer.buffer.get_unchecked(offset) };
                let value: ::core::result::Result<Self, _> =
                    ::core::convert::TryInto::try_into(value);
                value.map_err(|error_kind| {
                    error_kind.with_error_location(
                        "FixKind",
                        "VectorRead::from_buffer",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<FixKind> for FixKind {
            const STRIDE: usize = 1;

            type Value = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self {
                *self
            }

            #[inline]
            unsafe fn write_values(
                values: &[Self],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 1];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - i as u32,
                    );
                }
            }
        }

        /// The table `GpsFix` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `GpsFix` in the file `schema/messages.fbs:59`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct GpsFix {
            /// The field `t_ms` in the table `GpsFix`
            pub t_ms: u64,
            /// The field `lat_e7` in the table `GpsFix`
            pub lat_e7: i32,
            /// The field `lon_e7` in the table `GpsFix`
            pub lon_e7: i32,
            /// The field `alt_mm` in the table `GpsFix`
            pub alt_mm: i32,
            /// The field `speed_mm_s` in the table `GpsFix`
            pub speed_mm_s: u32,
            /// The field `hdop_centi` in the table `GpsFix`
            pub hdop_centi: u16,
            /// The field `sats` in the table `GpsFix`
            pub sats: u8,
            /// The field `fix` in the table `GpsFix`
            pub fix: self::FixKind,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for GpsFix {
            fn default() -> Self {
                Self {
                    t_ms: 0,
                    lat_e7: 0,
                    lon_e7: 0,
                    alt_mm: 0,
                    speed_mm_s: 0,
                    hdop_centi: 0,
                    sats: 0,
                    fix: self::FixKind::NoFix,
                }
            }
        }

        impl GpsFix {
            /// Creates a [GpsFixBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> GpsFixBuilder<()> {
                GpsFixBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_t_ms: impl ::planus::WriteAsDefault<u64, u64>,
                field_lat_e7: impl ::planus::WriteAsDefault<i32, i32>,
                field_lon_e7: impl ::planus::WriteAsDefault<i32, i32>,
                field_alt_mm: impl ::planus::WriteAsDefault<i32, i32>,
                field_speed_mm_s: impl ::planus::WriteAsDefault<u32, u32>,
                field_hdop_centi: impl ::planus::WriteAsDefault<u16, u16>,
                field_sats: impl ::planus::WriteAsDefault<u8, u8>,
                field_fix: impl ::planus::WriteAsDefault<self::FixKind, self::FixKind>,
            ) -> ::planus::Offset<Self> {
                let prepared_t_ms = field_t_ms.prepare(builder, &0);
                let prepared_lat_e7 = field_lat_e7.prepare(builder, &0);
                let prepared_lon_e7 = field_lon_e7.prepare(builder, &0);
                let prepared_alt_mm = field_alt_mm.prepare(builder, &0);
                let prepared_speed_mm_s = field_speed_mm_s.prepare(builder, &0);
                let prepared_hdop_centi = field_hdop_centi.prepare(builder, &0);
                let prepared_sats = field_sats.prepare(builder, &0);
                let prepared_fix = field_fix.prepare(builder, &self::FixKind::NoFix);

                let mut table_writer: ::planus::table_writer::TableWriter<20> =
                    ::core::default::Default::default();
                if prepared_t_ms.is_some() {
                    table_writer.write_entry::<u64>(0);
                }
                if prepared_lat_e7.is_some() {
                    table_writer.write_entry::<i32>(1);
                }
                if prepared_lon_e7.is_some() {
                    table_writer.write_entry::<i32>(2);
                }
                if prepared_alt_mm.is_some() {
                    table_writer.write_entry::<i32>(3);
                }
                if prepared_speed_mm_s.is_some() {
                    table_writer.write_entry::<u32>(4);
                }
                if prepared_hdop_centi.is_some() {
                    table_writer.write_entry::<u16>(5);
                }
                if prepared_sats.is_some() {
                    table_writer.write_entry::<u8>(6);
                }
                if prepared_fix.is_some() {
                    table_writer.write_entry::<self::FixKind>(7);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_t_ms) = prepared_t_ms {
                            object_writer.write::<_, _, 8>(&prepared_t_ms);
                        }
                        if let ::core::option::Option::Some(prepared_lat_e7) = prepared_lat_e7 {
                            object_writer.write::<_, _, 4>(&prepared_lat_e7);
                        }
                        if let ::core::option::Option::Some(prepared_lon_e7) = prepared_lon_e7 {
                            object_writer.write::<_, _, 4>(&prepared_lon_e7);
                        }
                        if let ::core::option::Option::Some(prepared_alt_mm) = prepared_alt_mm {
                            object_writer.write::<_, _, 4>(&prepared_alt_mm);
                        }
                        if let ::core::option::Option::Some(prepared_speed_mm_s) =
                            prepared_speed_mm_s
                        {
                            object_writer.write::<_, _, 4>(&prepared_speed_mm_s);
                        }
                        if let ::core::option::Option::Some(prepared_hdop_centi) =
                            prepared_hdop_centi
                        {
                            object_writer.write::<_, _, 2>(&prepared_hdop_centi);
                        }
                        if let ::core::option::Option::Some(prepared_sats) = prepared_sats {
                            object_writer.write::<_, _, 1>(&prepared_sats);
                        }
                        if let ::core::option::Option::Some(prepared_fix) = prepared_fix {
                            object_writer.write::<_, _, 1>(&prepared_fix);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<GpsFix>> for GpsFix {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GpsFix> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<GpsFix>> for GpsFix {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GpsFix>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<GpsFix> for GpsFix {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GpsFix> {
                GpsFix::create(
                    builder,
                    self.t_ms,
                    self.lat_e7,
                    self.lon_e7,
                    self.alt_mm,
                    self.speed_mm_s,
                    self.hdop_centi,
                    self.sats,
                    self.fix,
                )
            }
        }

        /// Builder for serializing an instance of the [GpsFix] type.
        ///
        /// Can be created using the [GpsFix::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct GpsFixBuilder<State>(State);

        impl GpsFixBuilder<()> {
            /// Setter for the [`t_ms` field](GpsFix#structfield.t_ms).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_ms<T0>(self, value: T0) -> GpsFixBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u64, u64>,
            {
                GpsFixBuilder((value,))
            }

            /// Sets the [`t_ms` field](GpsFix#structfield.t_ms) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn t_ms_as_default(self) -> GpsFixBuilder<(::planus::DefaultValue,)> {
                self.t_ms(::planus::DefaultValue)
            }
        }

        impl<T0> GpsFixBuilder<(T0,)> {
            /// Setter for the [`lat_e7` field](GpsFix#structfield.lat_e7).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn lat_e7<T1>(self, value: T1) -> GpsFixBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsDefault<i32, i32>,
            {
                let (v0,) = self.0;
                GpsFixBuilder((v0, value))
            }

            /// Sets the [`lat_e7` field](GpsFix#structfield.lat_e7) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn lat_e7_as_default(self) -> GpsFixBuilder<(T0, ::planus::DefaultValue)> {
                self.lat_e7(::planus::DefaultValue)
            }
        }

        impl<T0, T1> GpsFixBuilder<(T0, T1)> {
            /// Setter for the [`lon_e7` field](GpsFix#structfield.lon_e7).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn lon_e7<T2>(self, value: T2) -> GpsFixBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsDefault<i32, i32>,
            {
                let (v0, v1) = self.0;
                GpsFixBuilder((v0, v1, value))
            }

            /// Sets the [`lon_e7` field](GpsFix#structfield.lon_e7) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn lon_e7_as_default(self) -> GpsFixBuilder<(T0, T1, ::planus::DefaultValue)> {
                self.lon_e7(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2> GpsFixBuilder<(T0, T1, T2)> {
            /// Setter for the [`alt_mm` field](GpsFix#structfield.alt_mm).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn alt_mm<T3>(self, value: T3) -> GpsFixBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsDefault<i32, i32>,
            {
                let (v0, v1, v2) = self.0;
                GpsFixBuilder((v0, v1, v2, value))
            }

            /// Sets the [`alt_mm` field](GpsFix#structfield.alt_mm) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn alt_mm_as_default(self) -> GpsFixBuilder<(T0, T1, T2, ::planus::DefaultValue)> {
                self.alt_mm(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3> GpsFixBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`speed_mm_s` field](GpsFix#structfield.speed_mm_s).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn speed_mm_s<T4>(self, value: T4) -> GpsFixBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsDefault<u32, u32>,
            {
                let (v0, v1, v2, v3) = self.0;
                GpsFixBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`speed_mm_s` field](GpsFix#structfield.speed_mm_s) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn speed_mm_s_as_default(
                self,
            ) -> GpsFixBuilder<(T0, T1, T2, T3, ::planus::DefaultValue)> {
                self.speed_mm_s(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4> GpsFixBuilder<(T0, T1, T2, T3, T4)> {
            /// Setter for the [`hdop_centi` field](GpsFix#structfield.hdop_centi).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn hdop_centi<T5>(self, value: T5) -> GpsFixBuilder<(T0, T1, T2, T3, T4, T5)>
            where
                T5: ::planus::WriteAsDefault<u16, u16>,
            {
                let (v0, v1, v2, v3, v4) = self.0;
                GpsFixBuilder((v0, v1, v2, v3, v4, value))
            }

            /// Sets the [`hdop_centi` field](GpsFix#structfield.hdop_centi) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn hdop_centi_as_default(
                self,
            ) -> GpsFixBuilder<(T0, T1, T2, T3, T4, ::planus::DefaultValue)> {
                self.hdop_centi(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4, T5> GpsFixBuilder<(T0, T1, T2, T3, T4, T5)> {
            /// Setter for the [`sats` field](GpsFix#structfield.sats).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn sats<T6>(self, value: T6) -> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6)>
            where
                T6: ::planus::WriteAsDefault<u8, u8>,
            {
                let (v0, v1, v2, v3, v4, v5) = self.0;
                GpsFixBuilder((v0, v1, v2, v3, v4, v5, value))
            }

            /// Sets the [`sats` field](GpsFix#structfield.sats) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn sats_as_default(
                self,
            ) -> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, ::planus::DefaultValue)> {
                self.sats(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6)> {
            /// Setter for the [`fix` field](GpsFix#structfield.fix).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn fix<T7>(self, value: T7) -> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
            where
                T7: ::planus::WriteAsDefault<self::FixKind, self::FixKind>,
            {
                let (v0, v1, v2, v3, v4, v5, v6) = self.0;
                GpsFixBuilder((v0, v1, v2, v3, v4, v5, v6, value))
            }

            /// Sets the [`fix` field](GpsFix#structfield.fix) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn fix_as_default(
                self,
            ) -> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, ::planus::DefaultValue)> {
                self.fix(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6, T7> GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [GpsFix].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<GpsFix>
            where
                Self: ::planus::WriteAsOffset<GpsFix>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<i32, i32>,
            T2: ::planus::WriteAsDefault<i32, i32>,
            T3: ::planus::WriteAsDefault<i32, i32>,
            T4: ::planus::WriteAsDefault<u32, u32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
            T6: ::planus::WriteAsDefault<u8, u8>,
            T7: ::planus::WriteAsDefault<self::FixKind, self::FixKind>,
        > ::planus::WriteAs<::planus::Offset<GpsFix>>
            for GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<GpsFix>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GpsFix> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<i32, i32>,
            T2: ::planus::WriteAsDefault<i32, i32>,
            T3: ::planus::WriteAsDefault<i32, i32>,
            T4: ::planus::WriteAsDefault<u32, u32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
            T6: ::planus::WriteAsDefault<u8, u8>,
            T7: ::planus::WriteAsDefault<self::FixKind, self::FixKind>,
        > ::planus::WriteAsOptional<::planus::Offset<GpsFix>>
            for GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<GpsFix>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GpsFix>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u64, u64>,
            T1: ::planus::WriteAsDefault<i32, i32>,
            T2: ::planus::WriteAsDefault<i32, i32>,
            T3: ::planus::WriteAsDefault<i32, i32>,
            T4: ::planus::WriteAsDefault<u32, u32>,
            T5: ::planus::WriteAsDefault<u16, u16>,
            T6: ::planus::WriteAsDefault<u8, u8>,
            T7: ::planus::WriteAsDefault<self::FixKind, self::FixKind>,
        > ::planus::WriteAsOffset<GpsFix> for GpsFixBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GpsFix> {
                let (v0, v1, v2, v3, v4, v5, v6, v7) = &self.0;
                GpsFix::create(builder, v0, v1, v2, v3, v4, v5, v6, v7)
            }
        }

        /// Reference to a deserialized [GpsFix].
        #[derive(Copy, Clone)]
        pub struct GpsFixRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> GpsFixRef<'a> {
            /// Getter for the [`t_ms` field](GpsFix#structfield.t_ms).
            #[inline]
            pub fn t_ms(&self) -> ::planus::Result<u64> {
                ::core::result::Result::Ok(self.0.access(0, "GpsFix", "t_ms")?.unwrap_or(0))
            }

            /// Getter for the [`lat_e7` field](GpsFix#structfield.lat_e7).
            #[inline]
            pub fn lat_e7(&self) -> ::planus::Result<i32> {
                ::core::result::Result::Ok(self.0.access(1, "GpsFix", "lat_e7")?.unwrap_or(0))
            }

            /// Getter for the [`lon_e7` field](GpsFix#structfield.lon_e7).
            #[inline]
            pub fn lon_e7(&self) -> ::planus::Result<i32> {
                ::core::result::Result::Ok(self.0.access(2, "GpsFix", "lon_e7")?.unwrap_or(0))
            }

            /// Getter for the [`alt_mm` field](GpsFix#structfield.alt_mm).
            #[inline]
            pub fn alt_mm(&self) -> ::planus::Result<i32> {
                ::core::result::Result::Ok(self.0.access(3, "GpsFix", "alt_mm")?.unwrap_or(0))
            }

            /// Getter for the [`speed_mm_s` field](GpsFix#structfield.speed_mm_s).
            #[inline]
            pub fn speed_mm_s(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(self.0.access(4, "GpsFix", "speed_mm_s")?.unwrap_or(0))
            }

            /// Getter for the [`hdop_centi` field](GpsFix#structfield.hdop_centi).
            #[inline]
            pub fn hdop_centi(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(self.0.access(5, "GpsFix", "hdop_centi")?.unwrap_or(0))
            }

            /// Getter for the [`sats` field](GpsFix#structfield.sats).
            #[inline]
            pub fn sats(&self) -> ::planus::Result<u8> {
                ::core::result::Result::Ok(self.0.access(6, "GpsFix", "sats")?.unwrap_or(0))
            }

            /// Getter for the [`fix` field](GpsFix#structfield.fix).
            #[inline]
            pub fn fix(&self) -> ::planus::Result<self::FixKind> {
                ::core::result::Result::Ok(
                    self.0
                        .access(7, "GpsFix", "fix")?
                        .unwrap_or(self::FixKind::NoFix),
                )
            }
        }

        impl<'a> ::core::fmt::Debug for GpsFixRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("GpsFixRef");
                f.field("t_ms", &self.t_ms());
                f.field("lat_e7", &self.lat_e7());
                f.field("lon_e7", &self.lon_e7());
                f.field("alt_mm", &self.alt_mm());
                f.field("speed_mm_s", &self.speed_mm_s());
                f.field("hdop_centi", &self.hdop_centi());
                f.field("sats", &self.sats());
                f.field("fix", &self.fix());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<GpsFixRef<'a>> for GpsFix {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: GpsFixRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    t_ms: ::core::convert::TryInto::try_into(value.t_ms()?)?,
                    lat_e7: ::core::convert::TryInto::try_into(value.lat_e7()?)?,
                    lon_e7: ::core::convert::TryInto::try_into(value.lon_e7()?)?,
                    alt_mm: ::core::convert::TryInto::try_into(value.alt_mm()?)?,
                    speed_mm_s: ::core::convert::TryInto::try_into(value.speed_mm_s()?)?,
                    hdop_centi: ::core::convert::TryInto::try_into(value.hdop_centi()?)?,
                    sats: ::core::convert::TryInto::try_into(value.sats()?)?,
                    fix: ::core::convert::TryInto::try_into(value.fix()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for GpsFixRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for GpsFixRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[GpsFixRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<GpsFix>> for GpsFix {
            type Value = ::planus::Offset<GpsFix>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<GpsFix>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for GpsFixRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[GpsFixRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `CanFrame` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `CanFrame` in the file `schema/messages.fbs:71`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct CanFrame {
            /// The field `id` in the table `CanFrame`
            pub id: u32,
            /// The field `extended` in the table `CanFrame`
            pub extended: bool,
            /// The field `dt_us` in the table `CanFrame`
            pub dt_us: u16,
            /// The field `data` in the table `CanFrame`
            pub data: ::core::option::Option<::planus::alloc::vec::Vec<u8>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for CanFrame {
            fn default() -> Self {
                Self {
                    id: 0,
                    extended: false,
                    dt_us: 0,
                    data: ::core::default::Default::default(),
                }
            }
        }

        impl CanFrame {
            /// Creates a [CanFrameBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> CanFrameBuilder<()> {
                CanFrameBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_id: impl ::planus::WriteAsDefault<u32, u32>,
                field_extended: impl ::planus::WriteAsDefault<bool, bool>,
                field_dt_us: impl ::planus::WriteAsDefault<u16, u16>,
                field_data: impl ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            ) -> ::planus::Offset<Self> {
                let prepared_id = field_id.prepare(builder, &0);
                let prepared_extended = field_extended.prepare(builder, &false);
                let prepared_dt_us = field_dt_us.prepare(builder, &0);
                let prepared_data = field_data.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<12> =
                    ::core::default::Default::default();
                if prepared_id.is_some() {
                    table_writer.write_entry::<u32>(0);
                }
                if prepared_data.is_some() {
                    table_writer.write_entry::<::planus::Offset<[u8]>>(3);
                }
                if prepared_dt_us.is_some() {
                    table_writer.write_entry::<u16>(2);
                }
                if prepared_extended.is_some() {
                    table_writer.write_entry::<bool>(1);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_id) = prepared_id {
                            object_writer.write::<_, _, 4>(&prepared_id);
                        }
                        if let ::core::option::Option::Some(prepared_data) = prepared_data {
                            object_writer.write::<_, _, 4>(&prepared_data);
                        }
                        if let ::core::option::Option::Some(prepared_dt_us) = prepared_dt_us {
                            object_writer.write::<_, _, 2>(&prepared_dt_us);
                        }
                        if let ::core::option::Option::Some(prepared_extended) = prepared_extended {
                            object_writer.write::<_, _, 1>(&prepared_extended);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<CanFrame>> for CanFrame {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanFrame> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<CanFrame>> for CanFrame {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CanFrame>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<CanFrame> for CanFrame {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanFrame> {
                CanFrame::create(builder, self.id, self.extended, self.dt_us, &self.data)
            }
        }

        /// Builder for serializing an instance of the [CanFrame] type.
        ///
        /// Can be created using the [CanFrame::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct CanFrameBuilder<State>(State);

        impl CanFrameBuilder<()> {
            /// Setter for the [`id` field](CanFrame#structfield.id).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn id<T0>(self, value: T0) -> CanFrameBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u32, u32>,
            {
                CanFrameBuilder((value,))
            }

            /// Sets the [`id` field](CanFrame#structfield.id) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn id_as_default(self) -> CanFrameBuilder<(::planus::DefaultValue,)> {
                self.id(::planus::DefaultValue)
            }
        }

        impl<T0> CanFrameBuilder<(T0,)> {
            /// Setter for the [`extended` field](CanFrame#structfield.extended).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn extended<T1>(self, value: T1) -> CanFrameBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsDefault<bool, bool>,
            {
                let (v0,) = self.0;
                CanFrameBuilder((v0, value))
            }

            /// Sets the [`extended` field](CanFrame#structfield.extended) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn extended_as_default(self) -> CanFrameBuilder<(T0, ::planus::DefaultValue)> {
                self.extended(::planus::DefaultValue)
            }
        }

        impl<T0, T1> CanFrameBuilder<(T0, T1)> {
            /// Setter for the [`dt_us` field](CanFrame#structfield.dt_us).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dt_us<T2>(self, value: T2) -> CanFrameBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsDefault<u16, u16>,
            {
                let (v0, v1) = self.0;
                CanFrameBuilder((v0, v1, value))
            }

            /// Sets the [`dt_us` field](CanFrame#structfield.dt_us) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dt_us_as_default(self) -> CanFrameBuilder<(T0, T1, ::planus::DefaultValue)> {
                self.dt_us(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2> CanFrameBuilder<(T0, T1, T2)> {
            /// Setter for the [`data` field](CanFrame#structfield.data).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn data<T3>(self, value: T3) -> CanFrameBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            {
                let (v0, v1, v2) = self.0;
                CanFrameBuilder((v0, v1, v2, value))
            }

            /// Sets the [`data` field](CanFrame#structfield.data) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn data_as_null(self) -> CanFrameBuilder<(T0, T1, T2, ())> {
                self.data(())
            }
        }

        impl<T0, T1, T2, T3> CanFrameBuilder<(T0, T1, T2, T3)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [CanFrame].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanFrame>
            where
                Self: ::planus::WriteAsOffset<CanFrame>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<bool, bool>,
            T2: ::planus::WriteAsDefault<u16, u16>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAs<::planus::Offset<CanFrame>> for CanFrameBuilder<(T0, T1, T2, T3)>
        {
            type Prepared = ::planus::Offset<CanFrame>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanFrame> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<bool, bool>,
            T2: ::planus::WriteAsDefault<u16, u16>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAsOptional<::planus::Offset<CanFrame>>
            for CanFrameBuilder<(T0, T1, T2, T3)>
        {
            type Prepared = ::planus::Offset<CanFrame>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CanFrame>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u32, u32>,
            T1: ::planus::WriteAsDefault<bool, bool>,
            T2: ::planus::WriteAsDefault<u16, u16>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
        > ::planus::WriteAsOffset<CanFrame> for CanFrameBuilder<(T0, T1, T2, T3)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanFrame> {
                let (v0, v1, v2, v3) = &self.0;
                CanFrame::create(builder, v0, v1, v2, v3)
            }
        }

        /// Reference to a deserialized [CanFrame].
        #[derive(Copy, Clone)]
        pub struct CanFrameRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> CanFrameRef<'a> {
            /// Getter for the [`id` field](CanFrame#structfield.id).
            #[inline]
            pub fn id(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(self.0.access(0, "CanFrame", "id")?.unwrap_or(0))
            }

            /// Getter for the [`extended` field](CanFrame#structfield.extended).
            #[inline]
            pub fn extended(&self) -> ::planus::Result<bool> {
                ::core::result::Result::Ok(
                    self.0.access(1, "CanFrame", "extended")?.unwrap_or(false),
                )
            }

            /// Getter for the [`dt_us` field](CanFrame#structfield.dt_us).
            #[inline]
            pub fn dt_us(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(self.0.access(2, "CanFrame", "dt_us")?.unwrap_or(0))
            }

            /// Getter for the [`data` field](CanFrame#structfield.data).
            #[inline]
            pub fn data(&self) -> ::planus::Result<::core::option::Option<&'a [u8]>> {
                self.0.access(3, "CanFrame", "data")
            }
        }

        impl<'a> ::core::fmt::Debug for CanFrameRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("CanFrameRef");
                f.field("id", &self.id());
                f.field("extended", &self.extended());
                f.field("dt_us", &self.dt_us());
                if let ::core::option::Option::Some(field_data) = self.data().transpose() {
                    f.field("data", &field_data);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<CanFrameRef<'a>> for CanFrame {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: CanFrameRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    id: ::core::convert::TryInto::try_into(value.id()?)?,
                    extended: ::core::convert::TryInto::try_into(value.extended()?)?,
                    dt_us: ::core::convert::TryInto::try_into(value.dt_us()?)?,
                    data: value.data()?.map(|v| v.to_vec()),
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for CanFrameRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for CanFrameRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[CanFrameRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<CanFrame>> for CanFrame {
            type Value = ::planus::Offset<CanFrame>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<CanFrame>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for CanFrameRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[CanFrameRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `CanBatch` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `CanBatch` in the file `schema/messages.fbs:78`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct CanBatch {
            /// The field `bus` in the table `CanBatch`
            pub bus: u8,
            /// The field `frames` in the table `CanBatch`
            pub frames: ::core::option::Option<::planus::alloc::vec::Vec<self::CanFrame>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for CanBatch {
            fn default() -> Self {
                Self {
                    bus: 0,
                    frames: ::core::default::Default::default(),
                }
            }
        }

        impl CanBatch {
            /// Creates a [CanBatchBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> CanBatchBuilder<()> {
                CanBatchBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_bus: impl ::planus::WriteAsDefault<u8, u8>,
                field_frames: impl ::planus::WriteAsOptional<
                    ::planus::Offset<[::planus::Offset<self::CanFrame>]>,
                >,
            ) -> ::planus::Offset<Self> {
                let prepared_bus = field_bus.prepare(builder, &0);
                let prepared_frames = field_frames.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<8> =
                    ::core::default::Default::default();
                if prepared_frames.is_some() {
                    table_writer
                        .write_entry::<::planus::Offset<[::planus::Offset<self::CanFrame>]>>(1);
                }
                if prepared_bus.is_some() {
                    table_writer.write_entry::<u8>(0);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_frames) = prepared_frames {
                            object_writer.write::<_, _, 4>(&prepared_frames);
                        }
                        if let ::core::option::Option::Some(prepared_bus) = prepared_bus {
                            object_writer.write::<_, _, 1>(&prepared_bus);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<CanBatch>> for CanBatch {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanBatch> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<CanBatch>> for CanBatch {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CanBatch>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<CanBatch> for CanBatch {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanBatch> {
                CanBatch::create(builder, self.bus, &self.frames)
            }
        }

        /// Builder for serializing an instance of the [CanBatch] type.
        ///
        /// Can be created using the [CanBatch::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct CanBatchBuilder<State>(State);

        impl CanBatchBuilder<()> {
            /// Setter for the [`bus` field](CanBatch#structfield.bus).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn bus<T0>(self, value: T0) -> CanBatchBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u8, u8>,
            {
                CanBatchBuilder((value,))
            }

            /// Sets the [`bus` field](CanBatch#structfield.bus) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn bus_as_default(self) -> CanBatchBuilder<(::planus::DefaultValue,)> {
                self.bus(::planus::DefaultValue)
            }
        }

        impl<T0> CanBatchBuilder<(T0,)> {
            /// Setter for the [`frames` field](CanBatch#structfield.frames).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn frames<T1>(self, value: T1) -> CanBatchBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<self::CanFrame>]>>,
            {
                let (v0,) = self.0;
                CanBatchBuilder((v0, value))
            }

            /// Sets the [`frames` field](CanBatch#structfield.frames) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn frames_as_null(self) -> CanBatchBuilder<(T0, ())> {
                self.frames(())
            }
        }

        impl<T0, T1> CanBatchBuilder<(T0, T1)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [CanBatch].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanBatch>
            where
                Self: ::planus::WriteAsOffset<CanBatch>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u8, u8>,
            T1: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<self::CanFrame>]>>,
        > ::planus::WriteAs<::planus::Offset<CanBatch>> for CanBatchBuilder<(T0, T1)>
        {
            type Prepared = ::planus::Offset<CanBatch>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanBatch> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u8, u8>,
            T1: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<self::CanFrame>]>>,
        > ::planus::WriteAsOptional<::planus::Offset<CanBatch>> for CanBatchBuilder<(T0, T1)>
        {
            type Prepared = ::planus::Offset<CanBatch>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CanBatch>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u8, u8>,
            T1: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<self::CanFrame>]>>,
        > ::planus::WriteAsOffset<CanBatch> for CanBatchBuilder<(T0, T1)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CanBatch> {
                let (v0, v1) = &self.0;
                CanBatch::create(builder, v0, v1)
            }
        }

        /// Reference to a deserialized [CanBatch].
        #[derive(Copy, Clone)]
        pub struct CanBatchRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> CanBatchRef<'a> {
            /// Getter for the [`bus` field](CanBatch#structfield.bus).
            #[inline]
            pub fn bus(&self) -> ::planus::Result<u8> {
                ::core::result::Result::Ok(self.0.access(0, "CanBatch", "bus")?.unwrap_or(0))
            }

            /// Getter for the [`frames` field](CanBatch#structfield.frames).
            #[inline]
            pub fn frames(
                &self,
            ) -> ::planus::Result<
                ::core::option::Option<
                    ::planus::Vector<'a, ::planus::Result<self::CanFrameRef<'a>>>,
                >,
            > {
                self.0.access(1, "CanBatch", "frames")
            }
        }

        impl<'a> ::core::fmt::Debug for CanBatchRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("CanBatchRef");
                f.field("bus", &self.bus());
                if let ::core::option::Option::Some(field_frames) = self.frames().transpose() {
                    f.field("frames", &field_frames);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<CanBatchRef<'a>> for CanBatch {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: CanBatchRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    bus: ::core::convert::TryInto::try_into(value.bus()?)?,
                    frames: if let ::core::option::Option::Some(frames) = value.frames()? {
                        ::core::option::Option::Some(frames.to_vec_result()?)
                    } else {
                        ::core::option::Option::None
                    },
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for CanBatchRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for CanBatchRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[CanBatchRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<CanBatch>> for CanBatch {
            type Value = ::planus::Offset<CanBatch>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<CanBatch>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for CanBatchRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[CanBatchRef]", "read_as_root", 0)
                })
            }
        }

        /// The enum `Mode` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Enum `Mode` in the file `schema/messages.fbs:84`
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[repr(u8)]
        pub enum Mode {
            /// The variant `Idle` in the enum `Mode`
            Idle = 0,

            /// The variant `Init` in the enum `Mode`
            Init = 1,

            /// The variant `Running` in the enum `Mode`
            Running = 2,

            /// The variant `Paused` in the enum `Mode`
            Paused = 3,

            /// The variant `Fault` in the enum `Mode`
            Fault = 4,
        }

        impl Mode {
            /// Array containing all valid variants of Mode
            pub const ENUM_VALUES: [Self; 5] = [
                Self::Idle,
                Self::Init,
                Self::Running,
                Self::Paused,
                Self::Fault,
            ];
        }

        impl ::core::convert::TryFrom<u8> for Mode {
            type Error = ::planus::errors::UnknownEnumTagKind;
            #[inline]
            fn try_from(
                value: u8,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTagKind> {
                #[allow(clippy::match_single_binding)]
                match value {
                    0 => ::core::result::Result::Ok(Mode::Idle),
                    1 => ::core::result::Result::Ok(Mode::Init),
                    2 => ::core::result::Result::Ok(Mode::Running),
                    3 => ::core::result::Result::Ok(Mode::Paused),
                    4 => ::core::result::Result::Ok(Mode::Fault),

                    _ => ::core::result::Result::Err(::planus::errors::UnknownEnumTagKind {
                        tag: value as i128,
                    }),
                }
            }
        }

        impl ::core::convert::From<Mode> for u8 {
            #[inline]
            fn from(value: Mode) -> Self {
                value as u8
            }
        }

        /// # Safety
        /// The Planus compiler correctly calculates `ALIGNMENT` and `SIZE`.
        unsafe impl ::planus::Primitive for Mode {
            const ALIGNMENT: usize = 1;
            const SIZE: usize = 1;
        }

        impl ::planus::WriteAsPrimitive<Mode> for Mode {
            #[inline]
            fn write<const N: usize>(&self, cursor: ::planus::Cursor<'_, N>, buffer_position: u32) {
                (*self as u8).write(cursor, buffer_position);
            }
        }

        impl ::planus::WriteAs<Mode> for Mode {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Mode {
                *self
            }
        }

        impl ::planus::WriteAsDefault<Mode, Mode> for Mode {
            type Prepared = Self;

            #[inline]
            fn prepare(
                &self,
                _builder: &mut ::planus::Builder,
                default: &Mode,
            ) -> ::core::option::Option<Mode> {
                if self == default {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(*self)
                }
            }
        }

        impl ::planus::WriteAsOptional<Mode> for Mode {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> ::core::option::Option<Mode> {
                ::core::option::Option::Some(*self)
            }
        }

        impl<'buf> ::planus::TableRead<'buf> for Mode {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                let n: u8 = ::planus::TableRead::from_buffer(buffer, offset)?;
                ::core::result::Result::Ok(::core::convert::TryInto::try_into(n)?)
            }
        }

        impl<'buf> ::planus::VectorReadInner<'buf> for Mode {
            type Error = ::planus::errors::UnknownEnumTag;
            const STRIDE: usize = 1;
            #[inline]
            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTag> {
                let value = unsafe { *buffer.buffer.get_unchecked(offset) };
                let value: ::core::result::Result<Self, _> =
                    ::core::convert::TryInto::try_into(value);
                value.map_err(|error_kind| {
                    error_kind.with_error_location(
                        "Mode",
                        "VectorRead::from_buffer",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<Mode> for Mode {
            const STRIDE: usize = 1;

            type Value = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self {
                *self
            }

            #[inline]
            unsafe fn write_values(
                values: &[Self],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 1];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - i as u32,
                    );
                }
            }
        }

        /// The enum `Fault` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Enum `Fault` in the file `schema/messages.fbs:86`
        #[derive(
            Copy,
            Clone,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[repr(u8)]
        pub enum Fault {
            /// The variant `None` in the enum `Fault`
            None = 0,

            /// The variant `Overcurrent` in the enum `Fault`
            Overcurrent = 1,

            /// The variant `Overtemp` in the enum `Fault`
            Overtemp = 2,

            /// The variant `Undervoltage` in the enum `Fault`
            Undervoltage = 3,

            /// The variant `SensorLost` in the enum `Fault`
            SensorLost = 4,

            /// The variant `LinkLost` in the enum `Fault`
            LinkLost = 5,

            /// The variant `FlashEcc` in the enum `Fault`
            FlashEcc = 6,

            /// The variant `Watchdog` in the enum `Fault`
            Watchdog = 7,
        }

        impl Fault {
            /// Array containing all valid variants of Fault
            pub const ENUM_VALUES: [Self; 8] = [
                Self::None,
                Self::Overcurrent,
                Self::Overtemp,
                Self::Undervoltage,
                Self::SensorLost,
                Self::LinkLost,
                Self::FlashEcc,
                Self::Watchdog,
            ];
        }

        impl ::core::convert::TryFrom<u8> for Fault {
            type Error = ::planus::errors::UnknownEnumTagKind;
            #[inline]
            fn try_from(
                value: u8,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTagKind> {
                #[allow(clippy::match_single_binding)]
                match value {
                    0 => ::core::result::Result::Ok(Fault::None),
                    1 => ::core::result::Result::Ok(Fault::Overcurrent),
                    2 => ::core::result::Result::Ok(Fault::Overtemp),
                    3 => ::core::result::Result::Ok(Fault::Undervoltage),
                    4 => ::core::result::Result::Ok(Fault::SensorLost),
                    5 => ::core::result::Result::Ok(Fault::LinkLost),
                    6 => ::core::result::Result::Ok(Fault::FlashEcc),
                    7 => ::core::result::Result::Ok(Fault::Watchdog),

                    _ => ::core::result::Result::Err(::planus::errors::UnknownEnumTagKind {
                        tag: value as i128,
                    }),
                }
            }
        }

        impl ::core::convert::From<Fault> for u8 {
            #[inline]
            fn from(value: Fault) -> Self {
                value as u8
            }
        }

        /// # Safety
        /// The Planus compiler correctly calculates `ALIGNMENT` and `SIZE`.
        unsafe impl ::planus::Primitive for Fault {
            const ALIGNMENT: usize = 1;
            const SIZE: usize = 1;
        }

        impl ::planus::WriteAsPrimitive<Fault> for Fault {
            #[inline]
            fn write<const N: usize>(&self, cursor: ::planus::Cursor<'_, N>, buffer_position: u32) {
                (*self as u8).write(cursor, buffer_position);
            }
        }

        impl ::planus::WriteAs<Fault> for Fault {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Fault {
                *self
            }
        }

        impl ::planus::WriteAsDefault<Fault, Fault> for Fault {
            type Prepared = Self;

            #[inline]
            fn prepare(
                &self,
                _builder: &mut ::planus::Builder,
                default: &Fault,
            ) -> ::core::option::Option<Fault> {
                if self == default {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(*self)
                }
            }
        }

        impl ::planus::WriteAsOptional<Fault> for Fault {
            type Prepared = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> ::core::option::Option<Fault> {
                ::core::option::Option::Some(*self)
            }
        }

        impl<'buf> ::planus::TableRead<'buf> for Fault {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                let n: u8 = ::planus::TableRead::from_buffer(buffer, offset)?;
                ::core::result::Result::Ok(::core::convert::TryInto::try_into(n)?)
            }
        }

        impl<'buf> ::planus::VectorReadInner<'buf> for Fault {
            type Error = ::planus::errors::UnknownEnumTag;
            const STRIDE: usize = 1;
            #[inline]
            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'buf>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::UnknownEnumTag> {
                let value = unsafe { *buffer.buffer.get_unchecked(offset) };
                let value: ::core::result::Result<Self, _> =
                    ::core::convert::TryInto::try_into(value);
                value.map_err(|error_kind| {
                    error_kind.with_error_location(
                        "Fault",
                        "VectorRead::from_buffer",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<Fault> for Fault {
            const STRIDE: usize = 1;

            type Value = Self;

            #[inline]
            fn prepare(&self, _builder: &mut ::planus::Builder) -> Self {
                *self
            }

            #[inline]
            unsafe fn write_values(
                values: &[Self],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 1];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - i as u32,
                    );
                }
            }
        }

        /// The table `LinkDown` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `LinkDown` in the file `schema/messages.fbs:88`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct LinkDown {}

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for LinkDown {
            fn default() -> Self {
                Self {}
            }
        }

        impl LinkDown {
            /// Creates a [LinkDownBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> LinkDownBuilder<()> {
                LinkDownBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(builder: &mut ::planus::Builder) -> ::planus::Offset<Self> {
                let table_writer: ::planus::table_writer::TableWriter<4> =
                    ::core::default::Default::default();
                unsafe {
                    table_writer.finish(builder, |_table_writer| {});
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<LinkDown>> for LinkDown {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkDown> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<LinkDown>> for LinkDown {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkDown>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<LinkDown> for LinkDown {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkDown> {
                LinkDown::create(builder)
            }
        }

        /// Builder for serializing an instance of the [LinkDown] type.
        ///
        /// Can be created using the [LinkDown::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct LinkDownBuilder<State>(State);

        impl LinkDownBuilder<()> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [LinkDown].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkDown>
            where
                Self: ::planus::WriteAsOffset<LinkDown>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl ::planus::WriteAs<::planus::Offset<LinkDown>> for LinkDownBuilder<()> {
            type Prepared = ::planus::Offset<LinkDown>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkDown> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<LinkDown>> for LinkDownBuilder<()> {
            type Prepared = ::planus::Offset<LinkDown>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkDown>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<LinkDown> for LinkDownBuilder<()> {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkDown> {
                LinkDown::create(builder)
            }
        }

        /// Reference to a deserialized [LinkDown].
        #[derive(Copy, Clone)]
        pub struct LinkDownRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> LinkDownRef<'a> {}

        impl<'a> ::core::fmt::Debug for LinkDownRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("LinkDownRef");

                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<LinkDownRef<'a>> for LinkDown {
            type Error = ::planus::Error;

            fn try_from(_value: LinkDownRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {})
            }
        }

        impl<'a> ::planus::TableRead<'a> for LinkDownRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for LinkDownRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[LinkDownRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<LinkDown>> for LinkDown {
            type Value = ::planus::Offset<LinkDown>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<LinkDown>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for LinkDownRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[LinkDownRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `LinkUp` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `LinkUp` in the file `schema/messages.fbs:90`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct LinkUp {
            /// The field `mbps` in the table `LinkUp`
            pub mbps: u16,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for LinkUp {
            fn default() -> Self {
                Self { mbps: 0 }
            }
        }

        impl LinkUp {
            /// Creates a [LinkUpBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> LinkUpBuilder<()> {
                LinkUpBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_mbps: impl ::planus::WriteAsDefault<u16, u16>,
            ) -> ::planus::Offset<Self> {
                let prepared_mbps = field_mbps.prepare(builder, &0);

                let mut table_writer: ::planus::table_writer::TableWriter<6> =
                    ::core::default::Default::default();
                if prepared_mbps.is_some() {
                    table_writer.write_entry::<u16>(0);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_mbps) = prepared_mbps {
                            object_writer.write::<_, _, 2>(&prepared_mbps);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<LinkUp>> for LinkUp {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkUp> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<LinkUp>> for LinkUp {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkUp>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<LinkUp> for LinkUp {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkUp> {
                LinkUp::create(builder, self.mbps)
            }
        }

        /// Builder for serializing an instance of the [LinkUp] type.
        ///
        /// Can be created using the [LinkUp::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct LinkUpBuilder<State>(State);

        impl LinkUpBuilder<()> {
            /// Setter for the [`mbps` field](LinkUp#structfield.mbps).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn mbps<T0>(self, value: T0) -> LinkUpBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u16, u16>,
            {
                LinkUpBuilder((value,))
            }

            /// Sets the [`mbps` field](LinkUp#structfield.mbps) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn mbps_as_default(self) -> LinkUpBuilder<(::planus::DefaultValue,)> {
                self.mbps(::planus::DefaultValue)
            }
        }

        impl<T0> LinkUpBuilder<(T0,)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [LinkUp].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkUp>
            where
                Self: ::planus::WriteAsOffset<LinkUp>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<T0: ::planus::WriteAsDefault<u16, u16>> ::planus::WriteAs<::planus::Offset<LinkUp>>
            for LinkUpBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<LinkUp>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkUp> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<T0: ::planus::WriteAsDefault<u16, u16>>
            ::planus::WriteAsOptional<::planus::Offset<LinkUp>> for LinkUpBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<LinkUp>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkUp>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<T0: ::planus::WriteAsDefault<u16, u16>> ::planus::WriteAsOffset<LinkUp>
            for LinkUpBuilder<(T0,)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkUp> {
                let (v0,) = &self.0;
                LinkUp::create(builder, v0)
            }
        }

        /// Reference to a deserialized [LinkUp].
        #[derive(Copy, Clone)]
        pub struct LinkUpRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> LinkUpRef<'a> {
            /// Getter for the [`mbps` field](LinkUp#structfield.mbps).
            #[inline]
            pub fn mbps(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(self.0.access(0, "LinkUp", "mbps")?.unwrap_or(0))
            }
        }

        impl<'a> ::core::fmt::Debug for LinkUpRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("LinkUpRef");
                f.field("mbps", &self.mbps());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<LinkUpRef<'a>> for LinkUp {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: LinkUpRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    mbps: ::core::convert::TryInto::try_into(value.mbps()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for LinkUpRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for LinkUpRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[LinkUpRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<LinkUp>> for LinkUp {
            type Value = ::planus::Offset<LinkUp>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<LinkUp>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for LinkUpRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[LinkUpRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `LinkError` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `LinkError` in the file `schema/messages.fbs:94`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct LinkError {
            /// The field `code` in the table `LinkError`
            pub code: u8,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for LinkError {
            fn default() -> Self {
                Self { code: 0 }
            }
        }

        impl LinkError {
            /// Creates a [LinkErrorBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> LinkErrorBuilder<()> {
                LinkErrorBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_code: impl ::planus::WriteAsDefault<u8, u8>,
            ) -> ::planus::Offset<Self> {
                let prepared_code = field_code.prepare(builder, &0);

                let mut table_writer: ::planus::table_writer::TableWriter<6> =
                    ::core::default::Default::default();
                if prepared_code.is_some() {
                    table_writer.write_entry::<u8>(0);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_code) = prepared_code {
                            object_writer.write::<_, _, 1>(&prepared_code);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<LinkError>> for LinkError {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkError> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<LinkError>> for LinkError {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkError>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<LinkError> for LinkError {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkError> {
                LinkError::create(builder, self.code)
            }
        }

        /// Builder for serializing an instance of the [LinkError] type.
        ///
        /// Can be created using the [LinkError::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct LinkErrorBuilder<State>(State);

        impl LinkErrorBuilder<()> {
            /// Setter for the [`code` field](LinkError#structfield.code).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn code<T0>(self, value: T0) -> LinkErrorBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u8, u8>,
            {
                LinkErrorBuilder((value,))
            }

            /// Sets the [`code` field](LinkError#structfield.code) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn code_as_default(self) -> LinkErrorBuilder<(::planus::DefaultValue,)> {
                self.code(::planus::DefaultValue)
            }
        }

        impl<T0> LinkErrorBuilder<(T0,)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [LinkError].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkError>
            where
                Self: ::planus::WriteAsOffset<LinkError>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<T0: ::planus::WriteAsDefault<u8, u8>> ::planus::WriteAs<::planus::Offset<LinkError>>
            for LinkErrorBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<LinkError>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkError> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<T0: ::planus::WriteAsDefault<u8, u8>>
            ::planus::WriteAsOptional<::planus::Offset<LinkError>> for LinkErrorBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<LinkError>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<LinkError>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<T0: ::planus::WriteAsDefault<u8, u8>> ::planus::WriteAsOffset<LinkError>
            for LinkErrorBuilder<(T0,)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<LinkError> {
                let (v0,) = &self.0;
                LinkError::create(builder, v0)
            }
        }

        /// Reference to a deserialized [LinkError].
        #[derive(Copy, Clone)]
        pub struct LinkErrorRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> LinkErrorRef<'a> {
            /// Getter for the [`code` field](LinkError#structfield.code).
            #[inline]
            pub fn code(&self) -> ::planus::Result<u8> {
                ::core::result::Result::Ok(self.0.access(0, "LinkError", "code")?.unwrap_or(0))
            }
        }

        impl<'a> ::core::fmt::Debug for LinkErrorRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("LinkErrorRef");
                f.field("code", &self.code());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<LinkErrorRef<'a>> for LinkError {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: LinkErrorRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    code: ::core::convert::TryInto::try_into(value.code()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for LinkErrorRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for LinkErrorRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[LinkErrorRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<LinkError>> for LinkError {
            type Value = ::planus::Offset<LinkError>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<LinkError>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for LinkErrorRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[LinkErrorRef]", "read_as_root", 0)
                })
            }
        }

        /// The union `LinkState` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Union `LinkState` in the file `schema/messages.fbs:98`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub enum LinkState {
            /// The variant of type `LinkDown` in the union `LinkState`
            LinkDown(::planus::alloc::boxed::Box<self::LinkDown>),

            /// The variant of type `LinkUp` in the union `LinkState`
            LinkUp(::planus::alloc::boxed::Box<self::LinkUp>),

            /// The variant of type `LinkError` in the union `LinkState`
            LinkError(::planus::alloc::boxed::Box<self::LinkError>),
        }

        impl LinkState {
            /// Creates a [LinkStateBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> LinkStateBuilder<::planus::Uninitialized> {
                LinkStateBuilder(::planus::Uninitialized)
            }

            #[inline]
            pub fn create_link_down(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::LinkDown>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(1, value.prepare(builder).downcast())
            }

            #[inline]
            pub fn create_link_up(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::LinkUp>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(2, value.prepare(builder).downcast())
            }

            #[inline]
            pub fn create_link_error(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::LinkError>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(3, value.prepare(builder).downcast())
            }
        }

        impl ::planus::WriteAsUnion<LinkState> for LinkState {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<Self> {
                match self {
                    Self::LinkDown(value) => Self::create_link_down(builder, value),
                    Self::LinkUp(value) => Self::create_link_up(builder, value),
                    Self::LinkError(value) => Self::create_link_error(builder, value),
                }
            }
        }

        impl ::planus::WriteAsOptionalUnion<LinkState> for LinkState {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<Self>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Builder for serializing an instance of the [LinkState] type.
        ///
        /// Can be created using the [LinkState::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct LinkStateBuilder<T>(T);

        impl LinkStateBuilder<::planus::Uninitialized> {
            /// Creates an instance of the [`LinkDown` variant](LinkState#variant.LinkDown).
            #[inline]
            pub fn link_down<T>(self, value: T) -> LinkStateBuilder<::planus::Initialized<1, T>>
            where
                T: ::planus::WriteAsOffset<self::LinkDown>,
            {
                LinkStateBuilder(::planus::Initialized(value))
            }

            /// Creates an instance of the [`LinkUp` variant](LinkState#variant.LinkUp).
            #[inline]
            pub fn link_up<T>(self, value: T) -> LinkStateBuilder<::planus::Initialized<2, T>>
            where
                T: ::planus::WriteAsOffset<self::LinkUp>,
            {
                LinkStateBuilder(::planus::Initialized(value))
            }

            /// Creates an instance of the [`LinkError` variant](LinkState#variant.LinkError).
            #[inline]
            pub fn link_error<T>(self, value: T) -> LinkStateBuilder<::planus::Initialized<3, T>>
            where
                T: ::planus::WriteAsOffset<self::LinkError>,
            {
                LinkStateBuilder(::planus::Initialized(value))
            }
        }

        impl<const N: u8, T> LinkStateBuilder<::planus::Initialized<N, T>> {
            /// Finish writing the builder to get an [UnionOffset](::planus::UnionOffset) to a serialized [LinkState].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<LinkState>
            where
                Self: ::planus::WriteAsUnion<LinkState>,
            {
                ::planus::WriteAsUnion::prepare(&self, builder)
            }
        }

        impl<T> ::planus::WriteAsUnion<LinkState> for LinkStateBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkDown>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<LinkState> {
                ::planus::UnionOffset::new(1, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<LinkState> for LinkStateBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkDown>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<LinkState>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }
        impl<T> ::planus::WriteAsUnion<LinkState> for LinkStateBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkUp>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<LinkState> {
                ::planus::UnionOffset::new(2, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<LinkState> for LinkStateBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkUp>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<LinkState>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }
        impl<T> ::planus::WriteAsUnion<LinkState> for LinkStateBuilder<::planus::Initialized<3, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkError>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<LinkState> {
                ::planus::UnionOffset::new(3, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<LinkState> for LinkStateBuilder<::planus::Initialized<3, T>>
        where
            T: ::planus::WriteAsOffset<self::LinkError>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<LinkState>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Reference to a deserialized [LinkState].
        #[derive(Copy, Clone, Debug)]
        pub enum LinkStateRef<'a> {
            LinkDown(self::LinkDownRef<'a>),
            LinkUp(self::LinkUpRef<'a>),
            LinkError(self::LinkErrorRef<'a>),
        }

        impl<'a> ::core::convert::TryFrom<LinkStateRef<'a>> for LinkState {
            type Error = ::planus::Error;

            fn try_from(value: LinkStateRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(match value {
                    LinkStateRef::LinkDown(value) => {
                        Self::LinkDown(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }

                    LinkStateRef::LinkUp(value) => Self::LinkUp(::planus::alloc::boxed::Box::new(
                        ::core::convert::TryFrom::try_from(value)?,
                    )),

                    LinkStateRef::LinkError(value) => {
                        Self::LinkError(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }
                })
            }
        }

        impl<'a> ::planus::TableReadUnion<'a> for LinkStateRef<'a> {
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                tag: u8,
                field_offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                match tag {
                    1 => ::core::result::Result::Ok(Self::LinkDown(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    2 => ::core::result::Result::Ok(Self::LinkUp(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    3 => ::core::result::Result::Ok(Self::LinkError(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    _ => {
                        ::core::result::Result::Err(::planus::errors::ErrorKind::UnknownUnionTag {
                            tag,
                        })
                    }
                }
            }
        }

        impl<'a> ::planus::VectorReadUnion<'a> for LinkStateRef<'a> {
            const VECTOR_NAME: &'static str = "[LinkStateRef]";
        }

        /// The table `StateReport` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `StateReport` in the file `schema/messages.fbs:104`
        #[derive(
            Clone,
            Debug,
            PartialEq,
            PartialOrd,
            Eq,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct StateReport {
            /// The field `mode` in the table `StateReport`
            pub mode: self::Mode,
            /// The field `link` in the table `StateReport`
            pub link: ::core::option::Option<self::LinkState>,
            /// The field `faults` in the table `StateReport`
            pub faults: ::core::option::Option<::planus::alloc::vec::Vec<self::Fault>>,
            /// The field `battery_pct` in the table `StateReport`
            pub battery_pct: u8,
            /// The field `charging` in the table `StateReport`
            pub charging: bool,
            /// The field `uptime_s` in the table `StateReport`
            pub uptime_s: u32,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for StateReport {
            fn default() -> Self {
                Self {
                    mode: self::Mode::Idle,
                    link: ::core::default::Default::default(),
                    faults: ::core::default::Default::default(),
                    battery_pct: 0,
                    charging: false,
                    uptime_s: 0,
                }
            }
        }

        impl StateReport {
            /// Creates a [StateReportBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> StateReportBuilder<()> {
                StateReportBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_mode: impl ::planus::WriteAsDefault<self::Mode, self::Mode>,
                field_link: impl ::planus::WriteAsOptionalUnion<self::LinkState>,
                field_faults: impl ::planus::WriteAsOptional<::planus::Offset<[self::Fault]>>,
                field_battery_pct: impl ::planus::WriteAsDefault<u8, u8>,
                field_charging: impl ::planus::WriteAsDefault<bool, bool>,
                field_uptime_s: impl ::planus::WriteAsDefault<u32, u32>,
            ) -> ::planus::Offset<Self> {
                let prepared_mode = field_mode.prepare(builder, &self::Mode::Idle);
                let prepared_link = field_link.prepare(builder);
                let prepared_faults = field_faults.prepare(builder);
                let prepared_battery_pct = field_battery_pct.prepare(builder, &0);
                let prepared_charging = field_charging.prepare(builder, &false);
                let prepared_uptime_s = field_uptime_s.prepare(builder, &0);

                let mut table_writer: ::planus::table_writer::TableWriter<18> =
                    ::core::default::Default::default();
                if prepared_link.is_some() {
                    table_writer.write_entry::<::planus::Offset<self::LinkState>>(2);
                }
                if prepared_faults.is_some() {
                    table_writer.write_entry::<::planus::Offset<[self::Fault]>>(3);
                }
                if prepared_uptime_s.is_some() {
                    table_writer.write_entry::<u32>(6);
                }
                if prepared_mode.is_some() {
                    table_writer.write_entry::<self::Mode>(0);
                }
                if prepared_link.is_some() {
                    table_writer.write_entry::<u8>(1);
                }
                if prepared_battery_pct.is_some() {
                    table_writer.write_entry::<u8>(4);
                }
                if prepared_charging.is_some() {
                    table_writer.write_entry::<bool>(5);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_link) = prepared_link {
                            object_writer.write::<_, _, 4>(&prepared_link.offset());
                        }
                        if let ::core::option::Option::Some(prepared_faults) = prepared_faults {
                            object_writer.write::<_, _, 4>(&prepared_faults);
                        }
                        if let ::core::option::Option::Some(prepared_uptime_s) = prepared_uptime_s {
                            object_writer.write::<_, _, 4>(&prepared_uptime_s);
                        }
                        if let ::core::option::Option::Some(prepared_mode) = prepared_mode {
                            object_writer.write::<_, _, 1>(&prepared_mode);
                        }
                        if let ::core::option::Option::Some(prepared_link) = prepared_link {
                            object_writer.write::<_, _, 1>(&prepared_link.tag());
                        }
                        if let ::core::option::Option::Some(prepared_battery_pct) =
                            prepared_battery_pct
                        {
                            object_writer.write::<_, _, 1>(&prepared_battery_pct);
                        }
                        if let ::core::option::Option::Some(prepared_charging) = prepared_charging {
                            object_writer.write::<_, _, 1>(&prepared_charging);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<StateReport>> for StateReport {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<StateReport> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<StateReport>> for StateReport {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<StateReport>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<StateReport> for StateReport {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<StateReport> {
                StateReport::create(
                    builder,
                    self.mode,
                    &self.link,
                    &self.faults,
                    self.battery_pct,
                    self.charging,
                    self.uptime_s,
                )
            }
        }

        /// Builder for serializing an instance of the [StateReport] type.
        ///
        /// Can be created using the [StateReport::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct StateReportBuilder<State>(State);

        impl StateReportBuilder<()> {
            /// Setter for the [`mode` field](StateReport#structfield.mode).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn mode<T0>(self, value: T0) -> StateReportBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<self::Mode, self::Mode>,
            {
                StateReportBuilder((value,))
            }

            /// Sets the [`mode` field](StateReport#structfield.mode) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn mode_as_default(self) -> StateReportBuilder<(::planus::DefaultValue,)> {
                self.mode(::planus::DefaultValue)
            }
        }

        impl<T0> StateReportBuilder<(T0,)> {
            /// Setter for the [`link` field](StateReport#structfield.link).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn link<T1>(self, value: T1) -> StateReportBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsOptionalUnion<self::LinkState>,
            {
                let (v0,) = self.0;
                StateReportBuilder((v0, value))
            }

            /// Sets the [`link` field](StateReport#structfield.link) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn link_as_null(self) -> StateReportBuilder<(T0, ())> {
                self.link(())
            }
        }

        impl<T0, T1> StateReportBuilder<(T0, T1)> {
            /// Setter for the [`faults` field](StateReport#structfield.faults).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn faults<T2>(self, value: T2) -> StateReportBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsOptional<::planus::Offset<[self::Fault]>>,
            {
                let (v0, v1) = self.0;
                StateReportBuilder((v0, v1, value))
            }

            /// Sets the [`faults` field](StateReport#structfield.faults) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn faults_as_null(self) -> StateReportBuilder<(T0, T1, ())> {
                self.faults(())
            }
        }

        impl<T0, T1, T2> StateReportBuilder<(T0, T1, T2)> {
            /// Setter for the [`battery_pct` field](StateReport#structfield.battery_pct).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn battery_pct<T3>(self, value: T3) -> StateReportBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsDefault<u8, u8>,
            {
                let (v0, v1, v2) = self.0;
                StateReportBuilder((v0, v1, v2, value))
            }

            /// Sets the [`battery_pct` field](StateReport#structfield.battery_pct) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn battery_pct_as_default(
                self,
            ) -> StateReportBuilder<(T0, T1, T2, ::planus::DefaultValue)> {
                self.battery_pct(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3> StateReportBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`charging` field](StateReport#structfield.charging).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn charging<T4>(self, value: T4) -> StateReportBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsDefault<bool, bool>,
            {
                let (v0, v1, v2, v3) = self.0;
                StateReportBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`charging` field](StateReport#structfield.charging) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn charging_as_default(
                self,
            ) -> StateReportBuilder<(T0, T1, T2, T3, ::planus::DefaultValue)> {
                self.charging(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4> StateReportBuilder<(T0, T1, T2, T3, T4)> {
            /// Setter for the [`uptime_s` field](StateReport#structfield.uptime_s).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn uptime_s<T5>(self, value: T5) -> StateReportBuilder<(T0, T1, T2, T3, T4, T5)>
            where
                T5: ::planus::WriteAsDefault<u32, u32>,
            {
                let (v0, v1, v2, v3, v4) = self.0;
                StateReportBuilder((v0, v1, v2, v3, v4, value))
            }

            /// Sets the [`uptime_s` field](StateReport#structfield.uptime_s) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn uptime_s_as_default(
                self,
            ) -> StateReportBuilder<(T0, T1, T2, T3, T4, ::planus::DefaultValue)> {
                self.uptime_s(::planus::DefaultValue)
            }
        }

        impl<T0, T1, T2, T3, T4, T5> StateReportBuilder<(T0, T1, T2, T3, T4, T5)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [StateReport].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<StateReport>
            where
                Self: ::planus::WriteAsOffset<StateReport>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<self::Mode, self::Mode>,
            T1: ::planus::WriteAsOptionalUnion<self::LinkState>,
            T2: ::planus::WriteAsOptional<::planus::Offset<[self::Fault]>>,
            T3: ::planus::WriteAsDefault<u8, u8>,
            T4: ::planus::WriteAsDefault<bool, bool>,
            T5: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAs<::planus::Offset<StateReport>>
            for StateReportBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            type Prepared = ::planus::Offset<StateReport>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<StateReport> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<self::Mode, self::Mode>,
            T1: ::planus::WriteAsOptionalUnion<self::LinkState>,
            T2: ::planus::WriteAsOptional<::planus::Offset<[self::Fault]>>,
            T3: ::planus::WriteAsDefault<u8, u8>,
            T4: ::planus::WriteAsDefault<bool, bool>,
            T5: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAsOptional<::planus::Offset<StateReport>>
            for StateReportBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            type Prepared = ::planus::Offset<StateReport>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<StateReport>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<self::Mode, self::Mode>,
            T1: ::planus::WriteAsOptionalUnion<self::LinkState>,
            T2: ::planus::WriteAsOptional<::planus::Offset<[self::Fault]>>,
            T3: ::planus::WriteAsDefault<u8, u8>,
            T4: ::planus::WriteAsDefault<bool, bool>,
            T5: ::planus::WriteAsDefault<u32, u32>,
        > ::planus::WriteAsOffset<StateReport> for StateReportBuilder<(T0, T1, T2, T3, T4, T5)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<StateReport> {
                let (v0, v1, v2, v3, v4, v5) = &self.0;
                StateReport::create(builder, v0, v1, v2, v3, v4, v5)
            }
        }

        /// Reference to a deserialized [StateReport].
        #[derive(Copy, Clone)]
        pub struct StateReportRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> StateReportRef<'a> {
            /// Getter for the [`mode` field](StateReport#structfield.mode).
            #[inline]
            pub fn mode(&self) -> ::planus::Result<self::Mode> {
                ::core::result::Result::Ok(
                    self.0
                        .access(0, "StateReport", "mode")?
                        .unwrap_or(self::Mode::Idle),
                )
            }

            /// Getter for the [`link` field](StateReport#structfield.link).
            #[inline]
            pub fn link(&self) -> ::planus::Result<::core::option::Option<self::LinkStateRef<'a>>> {
                self.0.access_union(1, "StateReport", "link")
            }

            /// Getter for the [`faults` field](StateReport#structfield.faults).
            #[inline]
            pub fn faults(
                &self,
            ) -> ::planus::Result<
                ::core::option::Option<
                    ::planus::Vector<
                        'a,
                        ::core::result::Result<self::Fault, ::planus::errors::UnknownEnumTag>,
                    >,
                >,
            > {
                self.0.access(3, "StateReport", "faults")
            }

            /// Getter for the [`battery_pct` field](StateReport#structfield.battery_pct).
            #[inline]
            pub fn battery_pct(&self) -> ::planus::Result<u8> {
                ::core::result::Result::Ok(
                    self.0.access(4, "StateReport", "battery_pct")?.unwrap_or(0),
                )
            }

            /// Getter for the [`charging` field](StateReport#structfield.charging).
            #[inline]
            pub fn charging(&self) -> ::planus::Result<bool> {
                ::core::result::Result::Ok(
                    self.0
                        .access(5, "StateReport", "charging")?
                        .unwrap_or(false),
                )
            }

            /// Getter for the [`uptime_s` field](StateReport#structfield.uptime_s).
            #[inline]
            pub fn uptime_s(&self) -> ::planus::Result<u32> {
                ::core::result::Result::Ok(
                    self.0.access(6, "StateReport", "uptime_s")?.unwrap_or(0),
                )
            }
        }

        impl<'a> ::core::fmt::Debug for StateReportRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("StateReportRef");
                f.field("mode", &self.mode());
                if let ::core::option::Option::Some(field_link) = self.link().transpose() {
                    f.field("link", &field_link);
                }
                if let ::core::option::Option::Some(field_faults) = self.faults().transpose() {
                    f.field("faults", &field_faults);
                }
                f.field("battery_pct", &self.battery_pct());
                f.field("charging", &self.charging());
                f.field("uptime_s", &self.uptime_s());
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<StateReportRef<'a>> for StateReport {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: StateReportRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    mode: ::core::convert::TryInto::try_into(value.mode()?)?,
                    link: if let ::core::option::Option::Some(link) = value.link()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(link)?)
                    } else {
                        ::core::option::Option::None
                    },
                    faults: if let ::core::option::Option::Some(faults) = value.faults()? {
                        ::core::option::Option::Some(faults.to_vec_result()?)
                    } else {
                        ::core::option::Option::None
                    },
                    battery_pct: ::core::convert::TryInto::try_into(value.battery_pct()?)?,
                    charging: ::core::convert::TryInto::try_into(value.charging()?)?,
                    uptime_s: ::core::convert::TryInto::try_into(value.uptime_s()?)?,
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for StateReportRef<'a> {
            #[inline]
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                ::core::result::Result::Ok(Self(::planus::table_reader::Table::from_buffer(
                    buffer, offset,
                )?))
            }
        }

        impl<'a> ::planus::VectorReadInner<'a> for StateReportRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[StateReportRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<StateReport>> for StateReport {
            type Value = ::planus::Offset<StateReport>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<StateReport>],
                bytes: *mut ::core::mem::MaybeUninit<u8>,
                buffer_position: u32,
            ) {
                let bytes = bytes as *mut [::core::mem::MaybeUninit<u8>; 4];
                for (i, v) in ::core::iter::Iterator::enumerate(values.iter()) {
                    ::planus::WriteAsPrimitive::write(
                        v,
                        ::planus::Cursor::new(unsafe { &mut *bytes.add(i) }),
                        buffer_position - (Self::STRIDE * i) as u32,
                    );
                }
            }
        }

        impl<'a> ::planus::ReadAsRoot<'a> for StateReportRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[StateReportRef]", "read_as_root", 0)
                })
            }
        }
    }
}
