pub use root::*;

const _: () = ::planus::check_version_compatibility("planus-1.3.0");

/// The root namespace
///
/// Generated from these locations:
/// * File `schema/request.fbs`
#[no_implicit_prelude]
#[allow(clippy::needless_lifetimes)]
mod root {
    /// The namespace `wire_sizes`
    ///
    /// Generated from these locations:
    /// * File `schema/request.fbs`
    pub mod wire_sizes {
        /// The table `AbsolutePath` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `AbsolutePath` in the file `schema/request.fbs:8`
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
        pub struct AbsolutePath {
            /// The field `path` in the table `AbsolutePath`
            pub path: ::core::option::Option<::planus::alloc::vec::Vec<u32>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for AbsolutePath {
            fn default() -> Self {
                Self {
                    path: ::core::default::Default::default(),
                }
            }
        }

        impl AbsolutePath {
            /// Creates a [AbsolutePathBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> AbsolutePathBuilder<()> {
                AbsolutePathBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_path: impl ::planus::WriteAsOptional<::planus::Offset<[u32]>>,
            ) -> ::planus::Offset<Self> {
                let prepared_path = field_path.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<6> =
                    ::core::default::Default::default();
                if prepared_path.is_some() {
                    table_writer.write_entry::<::planus::Offset<[u32]>>(0);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_path) = prepared_path {
                            object_writer.write::<_, _, 4>(&prepared_path);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<AbsolutePath>> for AbsolutePath {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<AbsolutePath> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<AbsolutePath>> for AbsolutePath {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<AbsolutePath>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<AbsolutePath> for AbsolutePath {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<AbsolutePath> {
                AbsolutePath::create(builder, &self.path)
            }
        }

        /// Builder for serializing an instance of the [AbsolutePath] type.
        ///
        /// Can be created using the [AbsolutePath::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct AbsolutePathBuilder<State>(State);

        impl AbsolutePathBuilder<()> {
            /// Setter for the [`path` field](AbsolutePath#structfield.path).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn path<T0>(self, value: T0) -> AbsolutePathBuilder<(T0,)>
            where
                T0: ::planus::WriteAsOptional<::planus::Offset<[u32]>>,
            {
                AbsolutePathBuilder((value,))
            }

            /// Sets the [`path` field](AbsolutePath#structfield.path) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn path_as_null(self) -> AbsolutePathBuilder<((),)> {
                self.path(())
            }
        }

        impl<T0> AbsolutePathBuilder<(T0,)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [AbsolutePath].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<AbsolutePath>
            where
                Self: ::planus::WriteAsOffset<AbsolutePath>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u32]>>>
            ::planus::WriteAs<::planus::Offset<AbsolutePath>> for AbsolutePathBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<AbsolutePath>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<AbsolutePath> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u32]>>>
            ::planus::WriteAsOptional<::planus::Offset<AbsolutePath>>
            for AbsolutePathBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<AbsolutePath>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<AbsolutePath>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u32]>>>
            ::planus::WriteAsOffset<AbsolutePath> for AbsolutePathBuilder<(T0,)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<AbsolutePath> {
                let (v0,) = &self.0;
                AbsolutePath::create(builder, v0)
            }
        }

        /// Reference to a deserialized [AbsolutePath].
        #[derive(Copy, Clone)]
        pub struct AbsolutePathRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> AbsolutePathRef<'a> {
            /// Getter for the [`path` field](AbsolutePath#structfield.path).
            #[inline]
            pub fn path(
                &self,
            ) -> ::planus::Result<::core::option::Option<::planus::Vector<'a, u32>>> {
                self.0.access(0, "AbsolutePath", "path")
            }
        }

        impl<'a> ::core::fmt::Debug for AbsolutePathRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("AbsolutePathRef");
                if let ::core::option::Option::Some(field_path) = self.path().transpose() {
                    f.field("path", &field_path);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<AbsolutePathRef<'a>> for AbsolutePath {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: AbsolutePathRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    path: if let ::core::option::Option::Some(path) = value.path()? {
                        ::core::option::Option::Some(path.to_vec()?)
                    } else {
                        ::core::option::Option::None
                    },
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for AbsolutePathRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for AbsolutePathRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[AbsolutePathRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<AbsolutePath>> for AbsolutePath {
            type Value = ::planus::Offset<AbsolutePath>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<AbsolutePath>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for AbsolutePathRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[AbsolutePathRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `GlobalCompact` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `GlobalCompact` in the file `schema/request.fbs:12`
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
        pub struct GlobalCompact {}

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for GlobalCompact {
            fn default() -> Self {
                Self {}
            }
        }

        impl GlobalCompact {
            /// Creates a [GlobalCompactBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> GlobalCompactBuilder<()> {
                GlobalCompactBuilder(())
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

        impl ::planus::WriteAs<::planus::Offset<GlobalCompact>> for GlobalCompact {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalCompact> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<GlobalCompact>> for GlobalCompact {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GlobalCompact>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<GlobalCompact> for GlobalCompact {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalCompact> {
                GlobalCompact::create(builder)
            }
        }

        /// Builder for serializing an instance of the [GlobalCompact] type.
        ///
        /// Can be created using the [GlobalCompact::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct GlobalCompactBuilder<State>(State);

        impl GlobalCompactBuilder<()> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [GlobalCompact].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalCompact>
            where
                Self: ::planus::WriteAsOffset<GlobalCompact>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl ::planus::WriteAs<::planus::Offset<GlobalCompact>> for GlobalCompactBuilder<()> {
            type Prepared = ::planus::Offset<GlobalCompact>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalCompact> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<GlobalCompact>> for GlobalCompactBuilder<()> {
            type Prepared = ::planus::Offset<GlobalCompact>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GlobalCompact>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<GlobalCompact> for GlobalCompactBuilder<()> {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalCompact> {
                GlobalCompact::create(builder)
            }
        }

        /// Reference to a deserialized [GlobalCompact].
        #[derive(Copy, Clone)]
        pub struct GlobalCompactRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> GlobalCompactRef<'a> {}

        impl<'a> ::core::fmt::Debug for GlobalCompactRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("GlobalCompactRef");

                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<GlobalCompactRef<'a>> for GlobalCompact {
            type Error = ::planus::Error;

            fn try_from(_value: GlobalCompactRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {})
            }
        }

        impl<'a> ::planus::TableRead<'a> for GlobalCompactRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for GlobalCompactRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[GlobalCompactRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<GlobalCompact>> for GlobalCompact {
            type Value = ::planus::Offset<GlobalCompact>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<GlobalCompact>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for GlobalCompactRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[GlobalCompactRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `GlobalFull` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `GlobalFull` in the file `schema/request.fbs:14`
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
        pub struct GlobalFull {}

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for GlobalFull {
            fn default() -> Self {
                Self {}
            }
        }

        impl GlobalFull {
            /// Creates a [GlobalFullBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> GlobalFullBuilder<()> {
                GlobalFullBuilder(())
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

        impl ::planus::WriteAs<::planus::Offset<GlobalFull>> for GlobalFull {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalFull> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<GlobalFull>> for GlobalFull {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GlobalFull>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<GlobalFull> for GlobalFull {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalFull> {
                GlobalFull::create(builder)
            }
        }

        /// Builder for serializing an instance of the [GlobalFull] type.
        ///
        /// Can be created using the [GlobalFull::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct GlobalFullBuilder<State>(State);

        impl GlobalFullBuilder<()> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [GlobalFull].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalFull>
            where
                Self: ::planus::WriteAsOffset<GlobalFull>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl ::planus::WriteAs<::planus::Offset<GlobalFull>> for GlobalFullBuilder<()> {
            type Prepared = ::planus::Offset<GlobalFull>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalFull> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<GlobalFull>> for GlobalFullBuilder<()> {
            type Prepared = ::planus::Offset<GlobalFull>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<GlobalFull>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<GlobalFull> for GlobalFullBuilder<()> {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<GlobalFull> {
                GlobalFull::create(builder)
            }
        }

        /// Reference to a deserialized [GlobalFull].
        #[derive(Copy, Clone)]
        pub struct GlobalFullRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> GlobalFullRef<'a> {}

        impl<'a> ::core::fmt::Debug for GlobalFullRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("GlobalFullRef");

                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<GlobalFullRef<'a>> for GlobalFull {
            type Error = ::planus::Error;

            fn try_from(_value: GlobalFullRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {})
            }
        }

        impl<'a> ::planus::TableRead<'a> for GlobalFullRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for GlobalFullRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location(
                        "[GlobalFullRef]",
                        "get",
                        buffer.offset_from_start,
                    )
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<GlobalFull>> for GlobalFull {
            type Value = ::planus::Offset<GlobalFull>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<GlobalFull>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for GlobalFullRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[GlobalFullRef]", "read_as_root", 0)
                })
            }
        }

        /// The union `PathKind` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Union `PathKind` in the file `schema/request.fbs:16`
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
        pub enum PathKind {
            /// The variant of type `AbsolutePath` in the union `PathKind`
            AbsolutePath(::planus::alloc::boxed::Box<self::AbsolutePath>),

            /// The variant of type `GlobalCompact` in the union `PathKind`
            GlobalCompact(::planus::alloc::boxed::Box<self::GlobalCompact>),

            /// The variant of type `GlobalFull` in the union `PathKind`
            GlobalFull(::planus::alloc::boxed::Box<self::GlobalFull>),
        }

        impl PathKind {
            /// Creates a [PathKindBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> PathKindBuilder<::planus::Uninitialized> {
                PathKindBuilder(::planus::Uninitialized)
            }

            #[inline]
            pub fn create_absolute_path(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::AbsolutePath>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(1, value.prepare(builder).downcast())
            }

            #[inline]
            pub fn create_global_compact(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::GlobalCompact>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(2, value.prepare(builder).downcast())
            }

            #[inline]
            pub fn create_global_full(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::GlobalFull>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(3, value.prepare(builder).downcast())
            }
        }

        impl ::planus::WriteAsUnion<PathKind> for PathKind {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<Self> {
                match self {
                    Self::AbsolutePath(value) => Self::create_absolute_path(builder, value),
                    Self::GlobalCompact(value) => Self::create_global_compact(builder, value),
                    Self::GlobalFull(value) => Self::create_global_full(builder, value),
                }
            }
        }

        impl ::planus::WriteAsOptionalUnion<PathKind> for PathKind {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<Self>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Builder for serializing an instance of the [PathKind] type.
        ///
        /// Can be created using the [PathKind::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct PathKindBuilder<T>(T);

        impl PathKindBuilder<::planus::Uninitialized> {
            /// Creates an instance of the [`AbsolutePath` variant](PathKind#variant.AbsolutePath).
            #[inline]
            pub fn absolute_path<T>(self, value: T) -> PathKindBuilder<::planus::Initialized<1, T>>
            where
                T: ::planus::WriteAsOffset<self::AbsolutePath>,
            {
                PathKindBuilder(::planus::Initialized(value))
            }

            /// Creates an instance of the [`GlobalCompact` variant](PathKind#variant.GlobalCompact).
            #[inline]
            pub fn global_compact<T>(self, value: T) -> PathKindBuilder<::planus::Initialized<2, T>>
            where
                T: ::planus::WriteAsOffset<self::GlobalCompact>,
            {
                PathKindBuilder(::planus::Initialized(value))
            }

            /// Creates an instance of the [`GlobalFull` variant](PathKind#variant.GlobalFull).
            #[inline]
            pub fn global_full<T>(self, value: T) -> PathKindBuilder<::planus::Initialized<3, T>>
            where
                T: ::planus::WriteAsOffset<self::GlobalFull>,
            {
                PathKindBuilder(::planus::Initialized(value))
            }
        }

        impl<const N: u8, T> PathKindBuilder<::planus::Initialized<N, T>> {
            /// Finish writing the builder to get an [UnionOffset](::planus::UnionOffset) to a serialized [PathKind].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<PathKind>
            where
                Self: ::planus::WriteAsUnion<PathKind>,
            {
                ::planus::WriteAsUnion::prepare(&self, builder)
            }
        }

        impl<T> ::planus::WriteAsUnion<PathKind> for PathKindBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::AbsolutePath>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<PathKind> {
                ::planus::UnionOffset::new(1, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<PathKind> for PathKindBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::AbsolutePath>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<PathKind>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }
        impl<T> ::planus::WriteAsUnion<PathKind> for PathKindBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::GlobalCompact>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<PathKind> {
                ::planus::UnionOffset::new(2, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<PathKind> for PathKindBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::GlobalCompact>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<PathKind>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }
        impl<T> ::planus::WriteAsUnion<PathKind> for PathKindBuilder<::planus::Initialized<3, T>>
        where
            T: ::planus::WriteAsOffset<self::GlobalFull>,
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<PathKind> {
                ::planus::UnionOffset::new(3, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<PathKind> for PathKindBuilder<::planus::Initialized<3, T>>
        where
            T: ::planus::WriteAsOffset<self::GlobalFull>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<PathKind>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Reference to a deserialized [PathKind].
        #[derive(Copy, Clone, Debug)]
        pub enum PathKindRef<'a> {
            AbsolutePath(self::AbsolutePathRef<'a>),
            GlobalCompact(self::GlobalCompactRef<'a>),
            GlobalFull(self::GlobalFullRef<'a>),
        }

        impl<'a> ::core::convert::TryFrom<PathKindRef<'a>> for PathKind {
            type Error = ::planus::Error;

            fn try_from(value: PathKindRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(match value {
                    PathKindRef::AbsolutePath(value) => {
                        Self::AbsolutePath(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }

                    PathKindRef::GlobalCompact(value) => {
                        Self::GlobalCompact(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }

                    PathKindRef::GlobalFull(value) => {
                        Self::GlobalFull(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }
                })
            }
        }

        impl<'a> ::planus::TableReadUnion<'a> for PathKindRef<'a> {
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                tag: u8,
                field_offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                match tag {
                    1 => ::core::result::Result::Ok(Self::AbsolutePath(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    2 => ::core::result::Result::Ok(Self::GlobalCompact(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    3 => ::core::result::Result::Ok(Self::GlobalFull(
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

        impl<'a> ::planus::VectorReadUnion<'a> for PathKindRef<'a> {
            const VECTOR_NAME: &'static str = "[PathKindRef]";
        }

        /// The table `CallArgs` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `CallArgs` in the file `schema/request.fbs:22`
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
        pub struct CallArgs {
            /// The field `args` in the table `CallArgs`
            pub args: ::core::option::Option<::planus::alloc::vec::Vec<u8>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for CallArgs {
            fn default() -> Self {
                Self {
                    args: ::core::default::Default::default(),
                }
            }
        }

        impl CallArgs {
            /// Creates a [CallArgsBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> CallArgsBuilder<()> {
                CallArgsBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_args: impl ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            ) -> ::planus::Offset<Self> {
                let prepared_args = field_args.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<6> =
                    ::core::default::Default::default();
                if prepared_args.is_some() {
                    table_writer.write_entry::<::planus::Offset<[u8]>>(0);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_args) = prepared_args {
                            object_writer.write::<_, _, 4>(&prepared_args);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<CallArgs>> for CallArgs {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CallArgs> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<CallArgs>> for CallArgs {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CallArgs>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<CallArgs> for CallArgs {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CallArgs> {
                CallArgs::create(builder, &self.args)
            }
        }

        /// Builder for serializing an instance of the [CallArgs] type.
        ///
        /// Can be created using the [CallArgs::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct CallArgsBuilder<State>(State);

        impl CallArgsBuilder<()> {
            /// Setter for the [`args` field](CallArgs#structfield.args).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn args<T0>(self, value: T0) -> CallArgsBuilder<(T0,)>
            where
                T0: ::planus::WriteAsOptional<::planus::Offset<[u8]>>,
            {
                CallArgsBuilder((value,))
            }

            /// Sets the [`args` field](CallArgs#structfield.args) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn args_as_null(self) -> CallArgsBuilder<((),)> {
                self.args(())
            }
        }

        impl<T0> CallArgsBuilder<(T0,)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [CallArgs].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<CallArgs>
            where
                Self: ::planus::WriteAsOffset<CallArgs>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u8]>>>
            ::planus::WriteAs<::planus::Offset<CallArgs>> for CallArgsBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<CallArgs>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CallArgs> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u8]>>>
            ::planus::WriteAsOptional<::planus::Offset<CallArgs>> for CallArgsBuilder<(T0,)>
        {
            type Prepared = ::planus::Offset<CallArgs>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<CallArgs>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<T0: ::planus::WriteAsOptional<::planus::Offset<[u8]>>>
            ::planus::WriteAsOffset<CallArgs> for CallArgsBuilder<(T0,)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<CallArgs> {
                let (v0,) = &self.0;
                CallArgs::create(builder, v0)
            }
        }

        /// Reference to a deserialized [CallArgs].
        #[derive(Copy, Clone)]
        pub struct CallArgsRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> CallArgsRef<'a> {
            /// Getter for the [`args` field](CallArgs#structfield.args).
            #[inline]
            pub fn args(&self) -> ::planus::Result<::core::option::Option<&'a [u8]>> {
                self.0.access(0, "CallArgs", "args")
            }
        }

        impl<'a> ::core::fmt::Debug for CallArgsRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("CallArgsRef");
                if let ::core::option::Option::Some(field_args) = self.args().transpose() {
                    f.field("args", &field_args);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<CallArgsRef<'a>> for CallArgs {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: CallArgsRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    args: value.args()?.map(|v| v.to_vec()),
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for CallArgsRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for CallArgsRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[CallArgsRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<CallArgs>> for CallArgs {
            type Value = ::planus::Offset<CallArgs>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<CallArgs>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for CallArgsRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[CallArgsRef]", "read_as_root", 0)
                })
            }
        }

        /// The table `ReadKind` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `ReadKind` in the file `schema/request.fbs:26`
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
        pub struct ReadKind {}

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for ReadKind {
            fn default() -> Self {
                Self {}
            }
        }

        impl ReadKind {
            /// Creates a [ReadKindBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> ReadKindBuilder<()> {
                ReadKindBuilder(())
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

        impl ::planus::WriteAs<::planus::Offset<ReadKind>> for ReadKind {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<ReadKind> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<ReadKind>> for ReadKind {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<ReadKind>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<ReadKind> for ReadKind {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<ReadKind> {
                ReadKind::create(builder)
            }
        }

        /// Builder for serializing an instance of the [ReadKind] type.
        ///
        /// Can be created using the [ReadKind::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct ReadKindBuilder<State>(State);

        impl ReadKindBuilder<()> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [ReadKind].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<ReadKind>
            where
                Self: ::planus::WriteAsOffset<ReadKind>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl ::planus::WriteAs<::planus::Offset<ReadKind>> for ReadKindBuilder<()> {
            type Prepared = ::planus::Offset<ReadKind>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<ReadKind> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<ReadKind>> for ReadKindBuilder<()> {
            type Prepared = ::planus::Offset<ReadKind>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<ReadKind>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<ReadKind> for ReadKindBuilder<()> {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<ReadKind> {
                ReadKind::create(builder)
            }
        }

        /// Reference to a deserialized [ReadKind].
        #[derive(Copy, Clone)]
        pub struct ReadKindRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> ReadKindRef<'a> {}

        impl<'a> ::core::fmt::Debug for ReadKindRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("ReadKindRef");

                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<ReadKindRef<'a>> for ReadKind {
            type Error = ::planus::Error;

            fn try_from(_value: ReadKindRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {})
            }
        }

        impl<'a> ::planus::TableRead<'a> for ReadKindRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for ReadKindRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[ReadKindRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<ReadKind>> for ReadKind {
            type Value = ::planus::Offset<ReadKind>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<ReadKind>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for ReadKindRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[ReadKindRef]", "read_as_root", 0)
                })
            }
        }

        /// The union `RequestKind` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Union `RequestKind` in the file `schema/request.fbs:28`
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
        pub enum RequestKind {
            /// The variant of type `CallArgs` in the union `RequestKind`
            CallArgs(::planus::alloc::boxed::Box<self::CallArgs>),

            /// The variant of type `ReadKind` in the union `RequestKind`
            ReadKind(::planus::alloc::boxed::Box<self::ReadKind>),
        }

        impl RequestKind {
            /// Creates a [RequestKindBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> RequestKindBuilder<::planus::Uninitialized> {
                RequestKindBuilder(::planus::Uninitialized)
            }

            #[inline]
            pub fn create_call_args(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::CallArgs>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(1, value.prepare(builder).downcast())
            }

            #[inline]
            pub fn create_read_kind(
                builder: &mut ::planus::Builder,
                value: impl ::planus::WriteAsOffset<self::ReadKind>,
            ) -> ::planus::UnionOffset<Self> {
                ::planus::UnionOffset::new(2, value.prepare(builder).downcast())
            }
        }

        impl ::planus::WriteAsUnion<RequestKind> for RequestKind {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::UnionOffset<Self> {
                match self {
                    Self::CallArgs(value) => Self::create_call_args(builder, value),
                    Self::ReadKind(value) => Self::create_read_kind(builder, value),
                }
            }
        }

        impl ::planus::WriteAsOptionalUnion<RequestKind> for RequestKind {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<Self>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Builder for serializing an instance of the [RequestKind] type.
        ///
        /// Can be created using the [RequestKind::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct RequestKindBuilder<T>(T);

        impl RequestKindBuilder<::planus::Uninitialized> {
            /// Creates an instance of the [`CallArgs` variant](RequestKind#variant.CallArgs).
            #[inline]
            pub fn call_args<T>(self, value: T) -> RequestKindBuilder<::planus::Initialized<1, T>>
            where
                T: ::planus::WriteAsOffset<self::CallArgs>,
            {
                RequestKindBuilder(::planus::Initialized(value))
            }

            /// Creates an instance of the [`ReadKind` variant](RequestKind#variant.ReadKind).
            #[inline]
            pub fn read_kind<T>(self, value: T) -> RequestKindBuilder<::planus::Initialized<2, T>>
            where
                T: ::planus::WriteAsOffset<self::ReadKind>,
            {
                RequestKindBuilder(::planus::Initialized(value))
            }
        }

        impl<const N: u8, T> RequestKindBuilder<::planus::Initialized<N, T>> {
            /// Finish writing the builder to get an [UnionOffset](::planus::UnionOffset) to a serialized [RequestKind].
            #[inline]
            pub fn finish(
                self,
                builder: &mut ::planus::Builder,
            ) -> ::planus::UnionOffset<RequestKind>
            where
                Self: ::planus::WriteAsUnion<RequestKind>,
            {
                ::planus::WriteAsUnion::prepare(&self, builder)
            }
        }

        impl<T> ::planus::WriteAsUnion<RequestKind> for RequestKindBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::CallArgs>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::planus::UnionOffset<RequestKind> {
                ::planus::UnionOffset::new(1, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<RequestKind>
            for RequestKindBuilder<::planus::Initialized<1, T>>
        where
            T: ::planus::WriteAsOffset<self::CallArgs>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<RequestKind>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }
        impl<T> ::planus::WriteAsUnion<RequestKind> for RequestKindBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::ReadKind>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::planus::UnionOffset<RequestKind> {
                ::planus::UnionOffset::new(2, (self.0).0.prepare(builder).downcast())
            }
        }

        impl<T> ::planus::WriteAsOptionalUnion<RequestKind>
            for RequestKindBuilder<::planus::Initialized<2, T>>
        where
            T: ::planus::WriteAsOffset<self::ReadKind>,
        {
            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::UnionOffset<RequestKind>> {
                ::core::option::Option::Some(::planus::WriteAsUnion::prepare(self, builder))
            }
        }

        /// Reference to a deserialized [RequestKind].
        #[derive(Copy, Clone, Debug)]
        pub enum RequestKindRef<'a> {
            CallArgs(self::CallArgsRef<'a>),
            ReadKind(self::ReadKindRef<'a>),
        }

        impl<'a> ::core::convert::TryFrom<RequestKindRef<'a>> for RequestKind {
            type Error = ::planus::Error;

            fn try_from(value: RequestKindRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(match value {
                    RequestKindRef::CallArgs(value) => {
                        Self::CallArgs(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }

                    RequestKindRef::ReadKind(value) => {
                        Self::ReadKind(::planus::alloc::boxed::Box::new(
                            ::core::convert::TryFrom::try_from(value)?,
                        ))
                    }
                })
            }
        }

        impl<'a> ::planus::TableReadUnion<'a> for RequestKindRef<'a> {
            fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                tag: u8,
                field_offset: usize,
            ) -> ::core::result::Result<Self, ::planus::errors::ErrorKind> {
                match tag {
                    1 => ::core::result::Result::Ok(Self::CallArgs(
                        ::planus::TableRead::from_buffer(buffer, field_offset)?,
                    )),
                    2 => ::core::result::Result::Ok(Self::ReadKind(
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

        impl<'a> ::planus::VectorReadUnion<'a> for RequestKindRef<'a> {
            const VECTOR_NAME: &'static str = "[RequestKindRef]";
        }

        /// The table `Request` in the namespace `wire_sizes`
        ///
        /// Generated from these locations:
        /// * Table `Request` in the file `schema/request.fbs:33`
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
        pub struct Request {
            /// The field `seq` in the table `Request`
            pub seq: u16,
            /// The field `path_kind` in the table `Request`
            pub path_kind: ::core::option::Option<self::PathKind>,
            /// The field `request_kind` in the table `Request`
            pub request_kind: ::core::option::Option<self::RequestKind>,
            /// The field `flags` in the table `Request`
            pub flags: ::core::option::Option<::planus::alloc::vec::Vec<bool>>,
            /// The field `dummy_a` in the table `Request`
            pub dummy_a: ::core::option::Option<u8>,
            /// The field `dummy_b` in the table `Request`
            pub dummy_b: ::core::option::Option<u8>,
            /// The field `dummy_c` in the table `Request`
            pub dummy_c: ::core::option::Option<u8>,
            /// The field `strs` in the table `Request`
            pub strs:
                ::core::option::Option<::planus::alloc::vec::Vec<::planus::alloc::string::String>>,
        }

        #[allow(clippy::derivable_impls)]
        impl ::core::default::Default for Request {
            fn default() -> Self {
                Self {
                    seq: 0,
                    path_kind: ::core::default::Default::default(),
                    request_kind: ::core::default::Default::default(),
                    flags: ::core::default::Default::default(),
                    dummy_a: ::core::default::Default::default(),
                    dummy_b: ::core::default::Default::default(),
                    dummy_c: ::core::default::Default::default(),
                    strs: ::core::default::Default::default(),
                }
            }
        }

        impl Request {
            /// Creates a [RequestBuilder] for serializing an instance of this table.
            #[inline]
            pub fn builder() -> RequestBuilder<()> {
                RequestBuilder(())
            }

            #[allow(clippy::too_many_arguments)]
            pub fn create(
                builder: &mut ::planus::Builder,
                field_seq: impl ::planus::WriteAsDefault<u16, u16>,
                field_path_kind: impl ::planus::WriteAsOptionalUnion<self::PathKind>,
                field_request_kind: impl ::planus::WriteAsOptionalUnion<self::RequestKind>,
                field_flags: impl ::planus::WriteAsOptional<::planus::Offset<[bool]>>,
                field_dummy_a: impl ::planus::WriteAsOptional<u8>,
                field_dummy_b: impl ::planus::WriteAsOptional<u8>,
                field_dummy_c: impl ::planus::WriteAsOptional<u8>,
                field_strs: impl ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<str>]>>,
            ) -> ::planus::Offset<Self> {
                let prepared_seq = field_seq.prepare(builder, &0);
                let prepared_path_kind = field_path_kind.prepare(builder);
                let prepared_request_kind = field_request_kind.prepare(builder);
                let prepared_flags = field_flags.prepare(builder);
                let prepared_dummy_a = field_dummy_a.prepare(builder);
                let prepared_dummy_b = field_dummy_b.prepare(builder);
                let prepared_dummy_c = field_dummy_c.prepare(builder);
                let prepared_strs = field_strs.prepare(builder);

                let mut table_writer: ::planus::table_writer::TableWriter<24> =
                    ::core::default::Default::default();
                if prepared_path_kind.is_some() {
                    table_writer.write_entry::<::planus::Offset<self::PathKind>>(2);
                }
                if prepared_request_kind.is_some() {
                    table_writer.write_entry::<::planus::Offset<self::RequestKind>>(4);
                }
                if prepared_flags.is_some() {
                    table_writer.write_entry::<::planus::Offset<[bool]>>(5);
                }
                if prepared_strs.is_some() {
                    table_writer.write_entry::<::planus::Offset<[::planus::Offset<str>]>>(9);
                }
                if prepared_seq.is_some() {
                    table_writer.write_entry::<u16>(0);
                }
                if prepared_path_kind.is_some() {
                    table_writer.write_entry::<u8>(1);
                }
                if prepared_request_kind.is_some() {
                    table_writer.write_entry::<u8>(3);
                }
                if prepared_dummy_a.is_some() {
                    table_writer.write_entry::<u8>(6);
                }
                if prepared_dummy_b.is_some() {
                    table_writer.write_entry::<u8>(7);
                }
                if prepared_dummy_c.is_some() {
                    table_writer.write_entry::<u8>(8);
                }

                unsafe {
                    table_writer.finish(builder, |object_writer| {
                        if let ::core::option::Option::Some(prepared_path_kind) = prepared_path_kind
                        {
                            object_writer.write::<_, _, 4>(&prepared_path_kind.offset());
                        }
                        if let ::core::option::Option::Some(prepared_request_kind) =
                            prepared_request_kind
                        {
                            object_writer.write::<_, _, 4>(&prepared_request_kind.offset());
                        }
                        if let ::core::option::Option::Some(prepared_flags) = prepared_flags {
                            object_writer.write::<_, _, 4>(&prepared_flags);
                        }
                        if let ::core::option::Option::Some(prepared_strs) = prepared_strs {
                            object_writer.write::<_, _, 4>(&prepared_strs);
                        }
                        if let ::core::option::Option::Some(prepared_seq) = prepared_seq {
                            object_writer.write::<_, _, 2>(&prepared_seq);
                        }
                        if let ::core::option::Option::Some(prepared_path_kind) = prepared_path_kind
                        {
                            object_writer.write::<_, _, 1>(&prepared_path_kind.tag());
                        }
                        if let ::core::option::Option::Some(prepared_request_kind) =
                            prepared_request_kind
                        {
                            object_writer.write::<_, _, 1>(&prepared_request_kind.tag());
                        }
                        if let ::core::option::Option::Some(prepared_dummy_a) = prepared_dummy_a {
                            object_writer.write::<_, _, 1>(&prepared_dummy_a);
                        }
                        if let ::core::option::Option::Some(prepared_dummy_b) = prepared_dummy_b {
                            object_writer.write::<_, _, 1>(&prepared_dummy_b);
                        }
                        if let ::core::option::Option::Some(prepared_dummy_c) = prepared_dummy_c {
                            object_writer.write::<_, _, 1>(&prepared_dummy_c);
                        }
                    });
                }
                builder.current_offset()
            }
        }

        impl ::planus::WriteAs<::planus::Offset<Request>> for Request {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Request> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl ::planus::WriteAsOptional<::planus::Offset<Request>> for Request {
            type Prepared = ::planus::Offset<Self>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<Request>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl ::planus::WriteAsOffset<Request> for Request {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Request> {
                Request::create(
                    builder,
                    self.seq,
                    &self.path_kind,
                    &self.request_kind,
                    &self.flags,
                    self.dummy_a,
                    self.dummy_b,
                    self.dummy_c,
                    &self.strs,
                )
            }
        }

        /// Builder for serializing an instance of the [Request] type.
        ///
        /// Can be created using the [Request::builder] method.
        #[derive(Debug)]
        #[must_use]
        pub struct RequestBuilder<State>(State);

        impl RequestBuilder<()> {
            /// Setter for the [`seq` field](Request#structfield.seq).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn seq<T0>(self, value: T0) -> RequestBuilder<(T0,)>
            where
                T0: ::planus::WriteAsDefault<u16, u16>,
            {
                RequestBuilder((value,))
            }

            /// Sets the [`seq` field](Request#structfield.seq) to the default value.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn seq_as_default(self) -> RequestBuilder<(::planus::DefaultValue,)> {
                self.seq(::planus::DefaultValue)
            }
        }

        impl<T0> RequestBuilder<(T0,)> {
            /// Setter for the [`path_kind` field](Request#structfield.path_kind).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn path_kind<T1>(self, value: T1) -> RequestBuilder<(T0, T1)>
            where
                T1: ::planus::WriteAsOptionalUnion<self::PathKind>,
            {
                let (v0,) = self.0;
                RequestBuilder((v0, value))
            }

            /// Sets the [`path_kind` field](Request#structfield.path_kind) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn path_kind_as_null(self) -> RequestBuilder<(T0, ())> {
                self.path_kind(())
            }
        }

        impl<T0, T1> RequestBuilder<(T0, T1)> {
            /// Setter for the [`request_kind` field](Request#structfield.request_kind).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn request_kind<T2>(self, value: T2) -> RequestBuilder<(T0, T1, T2)>
            where
                T2: ::planus::WriteAsOptionalUnion<self::RequestKind>,
            {
                let (v0, v1) = self.0;
                RequestBuilder((v0, v1, value))
            }

            /// Sets the [`request_kind` field](Request#structfield.request_kind) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn request_kind_as_null(self) -> RequestBuilder<(T0, T1, ())> {
                self.request_kind(())
            }
        }

        impl<T0, T1, T2> RequestBuilder<(T0, T1, T2)> {
            /// Setter for the [`flags` field](Request#structfield.flags).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn flags<T3>(self, value: T3) -> RequestBuilder<(T0, T1, T2, T3)>
            where
                T3: ::planus::WriteAsOptional<::planus::Offset<[bool]>>,
            {
                let (v0, v1, v2) = self.0;
                RequestBuilder((v0, v1, v2, value))
            }

            /// Sets the [`flags` field](Request#structfield.flags) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn flags_as_null(self) -> RequestBuilder<(T0, T1, T2, ())> {
                self.flags(())
            }
        }

        impl<T0, T1, T2, T3> RequestBuilder<(T0, T1, T2, T3)> {
            /// Setter for the [`dummy_a` field](Request#structfield.dummy_a).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_a<T4>(self, value: T4) -> RequestBuilder<(T0, T1, T2, T3, T4)>
            where
                T4: ::planus::WriteAsOptional<u8>,
            {
                let (v0, v1, v2, v3) = self.0;
                RequestBuilder((v0, v1, v2, v3, value))
            }

            /// Sets the [`dummy_a` field](Request#structfield.dummy_a) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_a_as_null(self) -> RequestBuilder<(T0, T1, T2, T3, ())> {
                self.dummy_a(())
            }
        }

        impl<T0, T1, T2, T3, T4> RequestBuilder<(T0, T1, T2, T3, T4)> {
            /// Setter for the [`dummy_b` field](Request#structfield.dummy_b).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_b<T5>(self, value: T5) -> RequestBuilder<(T0, T1, T2, T3, T4, T5)>
            where
                T5: ::planus::WriteAsOptional<u8>,
            {
                let (v0, v1, v2, v3, v4) = self.0;
                RequestBuilder((v0, v1, v2, v3, v4, value))
            }

            /// Sets the [`dummy_b` field](Request#structfield.dummy_b) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_b_as_null(self) -> RequestBuilder<(T0, T1, T2, T3, T4, ())> {
                self.dummy_b(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5> RequestBuilder<(T0, T1, T2, T3, T4, T5)> {
            /// Setter for the [`dummy_c` field](Request#structfield.dummy_c).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_c<T6>(self, value: T6) -> RequestBuilder<(T0, T1, T2, T3, T4, T5, T6)>
            where
                T6: ::planus::WriteAsOptional<u8>,
            {
                let (v0, v1, v2, v3, v4, v5) = self.0;
                RequestBuilder((v0, v1, v2, v3, v4, v5, value))
            }

            /// Sets the [`dummy_c` field](Request#structfield.dummy_c) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn dummy_c_as_null(self) -> RequestBuilder<(T0, T1, T2, T3, T4, T5, ())> {
                self.dummy_c(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6> RequestBuilder<(T0, T1, T2, T3, T4, T5, T6)> {
            /// Setter for the [`strs` field](Request#structfield.strs).
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn strs<T7>(self, value: T7) -> RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
            where
                T7: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<str>]>>,
            {
                let (v0, v1, v2, v3, v4, v5, v6) = self.0;
                RequestBuilder((v0, v1, v2, v3, v4, v5, v6, value))
            }

            /// Sets the [`strs` field](Request#structfield.strs) to null.
            #[inline]
            #[allow(clippy::type_complexity)]
            pub fn strs_as_null(self) -> RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, ())> {
                self.strs(())
            }
        }

        impl<T0, T1, T2, T3, T4, T5, T6, T7> RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)> {
            /// Finish writing the builder to get an [Offset](::planus::Offset) to a serialized [Request].
            #[inline]
            pub fn finish(self, builder: &mut ::planus::Builder) -> ::planus::Offset<Request>
            where
                Self: ::planus::WriteAsOffset<Request>,
            {
                ::planus::WriteAsOffset::prepare(&self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsOptionalUnion<self::PathKind>,
            T2: ::planus::WriteAsOptionalUnion<self::RequestKind>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[bool]>>,
            T4: ::planus::WriteAsOptional<u8>,
            T5: ::planus::WriteAsOptional<u8>,
            T6: ::planus::WriteAsOptional<u8>,
            T7: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<str>]>>,
        > ::planus::WriteAs<::planus::Offset<Request>>
            for RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<Request>;

            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Request> {
                ::planus::WriteAsOffset::prepare(self, builder)
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsOptionalUnion<self::PathKind>,
            T2: ::planus::WriteAsOptionalUnion<self::RequestKind>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[bool]>>,
            T4: ::planus::WriteAsOptional<u8>,
            T5: ::planus::WriteAsOptional<u8>,
            T6: ::planus::WriteAsOptional<u8>,
            T7: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<str>]>>,
        > ::planus::WriteAsOptional<::planus::Offset<Request>>
            for RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            type Prepared = ::planus::Offset<Request>;

            #[inline]
            fn prepare(
                &self,
                builder: &mut ::planus::Builder,
            ) -> ::core::option::Option<::planus::Offset<Request>> {
                ::core::option::Option::Some(::planus::WriteAsOffset::prepare(self, builder))
            }
        }

        impl<
            T0: ::planus::WriteAsDefault<u16, u16>,
            T1: ::planus::WriteAsOptionalUnion<self::PathKind>,
            T2: ::planus::WriteAsOptionalUnion<self::RequestKind>,
            T3: ::planus::WriteAsOptional<::planus::Offset<[bool]>>,
            T4: ::planus::WriteAsOptional<u8>,
            T5: ::planus::WriteAsOptional<u8>,
            T6: ::planus::WriteAsOptional<u8>,
            T7: ::planus::WriteAsOptional<::planus::Offset<[::planus::Offset<str>]>>,
        > ::planus::WriteAsOffset<Request> for RequestBuilder<(T0, T1, T2, T3, T4, T5, T6, T7)>
        {
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> ::planus::Offset<Request> {
                let (v0, v1, v2, v3, v4, v5, v6, v7) = &self.0;
                Request::create(builder, v0, v1, v2, v3, v4, v5, v6, v7)
            }
        }

        /// Reference to a deserialized [Request].
        #[derive(Copy, Clone)]
        pub struct RequestRef<'a>(#[allow(dead_code)] ::planus::table_reader::Table<'a>);

        impl<'a> RequestRef<'a> {
            /// Getter for the [`seq` field](Request#structfield.seq).
            #[inline]
            pub fn seq(&self) -> ::planus::Result<u16> {
                ::core::result::Result::Ok(self.0.access(0, "Request", "seq")?.unwrap_or(0))
            }

            /// Getter for the [`path_kind` field](Request#structfield.path_kind).
            #[inline]
            pub fn path_kind(
                &self,
            ) -> ::planus::Result<::core::option::Option<self::PathKindRef<'a>>> {
                self.0.access_union(1, "Request", "path_kind")
            }

            /// Getter for the [`request_kind` field](Request#structfield.request_kind).
            #[inline]
            pub fn request_kind(
                &self,
            ) -> ::planus::Result<::core::option::Option<self::RequestKindRef<'a>>> {
                self.0.access_union(3, "Request", "request_kind")
            }

            /// Getter for the [`flags` field](Request#structfield.flags).
            #[inline]
            pub fn flags(
                &self,
            ) -> ::planus::Result<::core::option::Option<::planus::Vector<'a, bool>>> {
                self.0.access(5, "Request", "flags")
            }

            /// Getter for the [`dummy_a` field](Request#structfield.dummy_a).
            #[inline]
            pub fn dummy_a(&self) -> ::planus::Result<::core::option::Option<u8>> {
                self.0.access(6, "Request", "dummy_a")
            }

            /// Getter for the [`dummy_b` field](Request#structfield.dummy_b).
            #[inline]
            pub fn dummy_b(&self) -> ::planus::Result<::core::option::Option<u8>> {
                self.0.access(7, "Request", "dummy_b")
            }

            /// Getter for the [`dummy_c` field](Request#structfield.dummy_c).
            #[inline]
            pub fn dummy_c(&self) -> ::planus::Result<::core::option::Option<u8>> {
                self.0.access(8, "Request", "dummy_c")
            }

            /// Getter for the [`strs` field](Request#structfield.strs).
            #[inline]
            pub fn strs(
                &self,
            ) -> ::planus::Result<
                ::core::option::Option<
                    ::planus::Vector<'a, ::planus::Result<&'a ::core::primitive::str>>,
                >,
            > {
                self.0.access(9, "Request", "strs")
            }
        }

        impl<'a> ::core::fmt::Debug for RequestRef<'a> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut f = f.debug_struct("RequestRef");
                f.field("seq", &self.seq());
                if let ::core::option::Option::Some(field_path_kind) = self.path_kind().transpose()
                {
                    f.field("path_kind", &field_path_kind);
                }
                if let ::core::option::Option::Some(field_request_kind) =
                    self.request_kind().transpose()
                {
                    f.field("request_kind", &field_request_kind);
                }
                if let ::core::option::Option::Some(field_flags) = self.flags().transpose() {
                    f.field("flags", &field_flags);
                }
                if let ::core::option::Option::Some(field_dummy_a) = self.dummy_a().transpose() {
                    f.field("dummy_a", &field_dummy_a);
                }
                if let ::core::option::Option::Some(field_dummy_b) = self.dummy_b().transpose() {
                    f.field("dummy_b", &field_dummy_b);
                }
                if let ::core::option::Option::Some(field_dummy_c) = self.dummy_c().transpose() {
                    f.field("dummy_c", &field_dummy_c);
                }
                if let ::core::option::Option::Some(field_strs) = self.strs().transpose() {
                    f.field("strs", &field_strs);
                }
                f.finish()
            }
        }

        impl<'a> ::core::convert::TryFrom<RequestRef<'a>> for Request {
            type Error = ::planus::Error;

            #[allow(unreachable_code)]
            fn try_from(value: RequestRef<'a>) -> ::planus::Result<Self> {
                ::core::result::Result::Ok(Self {
                    seq: ::core::convert::TryInto::try_into(value.seq()?)?,
                    path_kind: if let ::core::option::Option::Some(path_kind) = value.path_kind()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(path_kind)?)
                    } else {
                        ::core::option::Option::None
                    },
                    request_kind: if let ::core::option::Option::Some(request_kind) =
                        value.request_kind()?
                    {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(
                            request_kind,
                        )?)
                    } else {
                        ::core::option::Option::None
                    },
                    flags: if let ::core::option::Option::Some(flags) = value.flags()? {
                        ::core::option::Option::Some(flags.to_vec()?)
                    } else {
                        ::core::option::Option::None
                    },
                    dummy_a: if let ::core::option::Option::Some(dummy_a) = value.dummy_a()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(dummy_a)?)
                    } else {
                        ::core::option::Option::None
                    },
                    dummy_b: if let ::core::option::Option::Some(dummy_b) = value.dummy_b()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(dummy_b)?)
                    } else {
                        ::core::option::Option::None
                    },
                    dummy_c: if let ::core::option::Option::Some(dummy_c) = value.dummy_c()? {
                        ::core::option::Option::Some(::core::convert::TryInto::try_into(dummy_c)?)
                    } else {
                        ::core::option::Option::None
                    },
                    strs: if let ::core::option::Option::Some(strs) = value.strs()? {
                        ::core::option::Option::Some(strs.to_vec_result()?)
                    } else {
                        ::core::option::Option::None
                    },
                })
            }
        }

        impl<'a> ::planus::TableRead<'a> for RequestRef<'a> {
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

        impl<'a> ::planus::VectorReadInner<'a> for RequestRef<'a> {
            type Error = ::planus::Error;
            const STRIDE: usize = 4;

            unsafe fn from_buffer(
                buffer: ::planus::SliceWithStartOffset<'a>,
                offset: usize,
            ) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(buffer, offset).map_err(|error_kind| {
                    error_kind.with_error_location("[RequestRef]", "get", buffer.offset_from_start)
                })
            }
        }

        /// # Safety
        /// The planus compiler generates implementations that initialize
        /// the bytes in `write_values`.
        unsafe impl ::planus::VectorWrite<::planus::Offset<Request>> for Request {
            type Value = ::planus::Offset<Request>;
            const STRIDE: usize = 4;
            #[inline]
            fn prepare(&self, builder: &mut ::planus::Builder) -> Self::Value {
                ::planus::WriteAs::prepare(self, builder)
            }

            #[inline]
            unsafe fn write_values(
                values: &[::planus::Offset<Request>],
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

        impl<'a> ::planus::ReadAsRoot<'a> for RequestRef<'a> {
            fn read_as_root(slice: &'a [u8]) -> ::planus::Result<Self> {
                ::planus::TableRead::from_buffer(
                    ::planus::SliceWithStartOffset {
                        buffer: slice,
                        offset_from_start: 0,
                    },
                    0,
                )
                .map_err(|error_kind| {
                    error_kind.with_error_location("[RequestRef]", "read_as_root", 0)
                })
            }
        }
    }
}
