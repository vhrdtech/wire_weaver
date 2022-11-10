# Very Hard Language / vhL
vhL is a language aimed at fully defining machine-to-machine communications. [TODO: write short and meaningful description]

vhL is not a regular language, it is (mostly) not designed to be executed, but rather it precisely describes what
needs to be done in another languages.

Primary targets are:
* Rust (specifically targeting embedded no_std environment)
* Dart+Flutter (UI) 
* `bytecode` (simple enough to be used by constrained embedded systems with partial functionality)
* C++ (compatibility and legacy reasons)
* HDL language (maybe SpinalHDL)
 
Current state is - highly experimental.

_Things are mostly sorted and to be implemented in the importance order._

Goals and features of the language:

## 1. Advanced type system / `types`
The goal is to develop a rich type system with first class support for bounded numbers and SI units that will be
able to represent even the most complex systems, while still being able to serialise everything in all
the supported targets.

### Features
* Bytecode target should be supported
* Provide `Default`, `Example` and `Allowed` values for any user defined type

### Types
* Boolean (true / false)
* Discrete numbers:
  * Signed: `i8` / `i16` / `i32` / `i64` / `i128`
  * Signed with configurable length: `i{expr -> u32}`
  * Unsigned: `u8`, `u16`, `u32`, `u64`, `u128`
  * Unsigned with configurable length: `u<expr -> u32>`
* Fixed point numbers (Q notation):
  * Signed: `q3.12`
  * Unsigned: `uq1.15`
  * Implicitly scaled: `q12<u8>` 0..=255 * 2^-12 (m=0, n=12)
  * Full syntax to allow constant's to be easily used: `q<expr -> (u32, u32)>`, `uq<expr -> (u32, u32)>`
* Floating point numbers:
  * `float32`, `float64` (IEEE-754)
  * ?`float16` and others
* Textual:
  * `char` (Unicode character)
  * `string` (UTF-8)
  * `c_char` (ASCII)
* Sequence:
  * Tuple
  * Array
    * `array<T, N>`: array of T with N elements
    * `binary<N>`: array with T=u8
    * Bounded array when N is bounded
* User-defined:
  * Struct
  * Enum
  * Union
  * Bitfield
* ? :
  * Map
* Variable length encoded numbers: `varint<Flavor>`, Flavor: gve, vlq, leb128, zigzag

### Bounded / checked numbers

Simple checked numbers where only a range of values is allowed:
* `u16 in 1..=512` - values 513 and above are not allowed
* or `u16<1..=512>` for consistency?
* `u<N>` or `i<N>` - valid values from -2^(N-1) to 2^(N-1)-1
* `u<N, [B]>` - where B is an optional bound
* ? auto-bound-number: `-10..21` to let vhL choose the smallest representation possible automatically
* `autonum` keyword: `autonum<-10..21>`, `autonum<0.00, 0.01 ..= 1.00>`
* fixed point can be derived as well: `-1.0,-0.9..1.0`
* `1..=512`, `0.00, 0.01 ..= 1.00` above is an implicit form of a special type - `numbound`

Set of allowed values:

* `u8 @{0..=8, 12, 16, 20, 24, 32, 48, 64}`

Custom checker function:

* `u8 @check_fun` where `fn check_fun(u8) -> bool`

Modulo numbers:

* `u8 @{mod 127}`

Mapped numbers - provide a function to map from one range to another. Modulo numbers are exactly that with fn being %N ?

Number classes/traits?:

* Natural: 1, 2, 3, 4, 5, ...
* Whole: 0, 1, 2, 3, 4, 5, ...
* Integer: ..., -3, -2, -1, 0, 1, 2, 3, ...
* ?Fraction: a/b, a and b are Whole, b != 0
* ? Rational: p/q, p and q are Integer, q != 0
* Real: Rational + Irrational
* Complex

Shifted or scaled numbers:
* Additional optional parameter (of the same type as the number itself) on all numbers representing shift/bias.
* u8<+1000> actual range = 1000..=1255 (10 bits), sent around range = 0 to 255 (8 bits).
* `+` or `-` required
* uq<(1, 15), *1e6>

Range analysis:
* Get a report of all numbers used with ranges, steps, allowed values, bits required: `vhl num report`

### Bounded array sizes
* Array type is `[T; numbound]`, 
where T is any type and `numbound` is a special type holding allowed values described above.

### SI support

Specify any SI unit for any number, define derived and custom units, define unit relations.
* current: q3.12 \`A`
* velocity: f32 \`m/s`

Units are enclosed inside a backtick character: `

Custom units with optional shortened name:
```
unit Byte = `1, "B"`;

unit Ki = `1024`;
unit Mi = `1024 * Ki`;
unit Gi = `1024 * Mi`;

unit Frame = `1`;
unit Packet = `1`;
```

Generic struct with related units, `rate` unit must be `U`'s unit divided by Seconds:
```
struct RateCounter<T: WholeNumber`U`> {
    current: T `U`, 
    rate: f32 `U/s`
}

struct ByteCounters {
  ingress: RateCounter<u64 `Byte`>,
  egress: RateCounter<u64 `Byte`>,
}

struct FrameCounters {
  ingress: RateCounter<u64 `Frame`>,
  egress: RateCounter<u64 `Frame`>,
}
```

### Naming convention
Only ASCII identifiers or Greek letters (will be converted into names in languages not supporting Unicode identifiers)
are allowed at this point.
* User types: CamelCase
* Constants: SNAKE_CASE_UPPER
* Functions: lower_case_snake

## 2. Serialisation and Deserialisation / `serdes`
Several wire formats targeted at constrained systems and low bandwidth channels, inter process communication and
ease of human editing are planned. See [Wire formats](book/src/wire_formats/wire_formats.md).


## 3. / `api`
Define hardware device or software service API/ABI as a collection primitives such as methods, properties,
streams and more.

### Features
* Semantic versioning for backwards compatibility checking
* Generate **server** side code with the ability for a user to provide an implementation
* Generate **client** side code that can be used to interact with server part
* Generate **test** side code (basically client or server code but tailored for tests and maybe in just one
target language)
* Ability to configure and tweak the code generator
* \[For streams or observable properties\] Specify additional information about rates, ability to configure emit rate
and figure out how to divide available channel bandwidth (**congestion control**).
* \[For properties\] - Ability to get several properties as one batch call. Ability to observe several properties as one batch.
Ability to specify congestion control policy between selected properties.
* ? Allow user to modify generated code. Automatically extract patches, store and apply them to account for
inevitable shortcomings of the codegen. 
* For now codegen to be implemented in Rust, maybe later by the language itself

### Syntax
* Resource definition begins with a `/` followed by a name of the resource, it's type and a declaration block:
  * `/acceleration<f32, '0> {}` where `'0` is a serial number that can be omitted (in which case it can be auto generated later).
* If type is omitted, then it's a group of resources: `/velocity {}`

* Declaration block can contain properties, nested resources or groups:
```
/accelerometer {
  /x<ro f32> {}
  /y<ro f32> {}
  /z<ro f32> {}
}
```

#### Resource tuple
Similar resources can be grouped into one declaration to avoid repeated code:
```
/accelerometer {
  /`'x'..='z'`<ro f32> {} 
}
```
or
```
/accelerometer {
  /`('x', 'y', 'z')`<ro f32> {} 
}
```
Ticked expression must resolve into either a:
* Range of numbers or char literals
* Or a tuple of numbers, char literals or strings

#### Resource array
It is also possible to define an array of resources:
```
/channels<[_; 8]> {
  /value<u8> {}
}
```
Bounded numbers are allowed:
```
/channels<[_; max 8]> {
  /value<u8> {}
}
```
In this case an actual number of resources available at the moment must be set during run time by a generated user code.

### Resource properties
* `description` - textual description of the resource function
* `default` - default value for a resource or constant
* `values` - array of named values, can be used to create enums in generated code and as examples in documentation
  * `values: [10 => "Nominal setting", 20 => "High setting"];`
  * `values: [10 => { description: "Nominal setting", ident: "NominalSetting" } ];`
  * Restrict resource to be only one of the `values: strict ["low", "mid", "high"]`

#### Properties specific to low level hardware registers
If a resource describes a hardware register in memory, additional properties are available:
* `addr` - address in memory.
* `interface` - separately defined interface block, through which all IO operations are performed.
* `endiannness` - `little` or `big`.
* `read_sideeffects` - set to true if read access results in an undesirable system state change,
for example reading data register of a peripheral can sometimes change status flags (`false` by default).
* `reserved` - not used portion of the bitfield, can also be set `ro` if it is forbidden to write anything else than default value into it.

Interface block can be defined as follows:
```
interface i2c0 {
  speed: 100 [kHz];
}
```
Actual interface management must be performed by the user code, properties defined will be provided to it.

#### Units
SI unit can be added after `bits` if the bit portion of the register represent a physical quantity.
If special conversion is required to obtain a physical value (shift bits around, bias, etc) - create an alias resource and provide an equation.

#### Bit fields
Specify a resource to be `bitfield<T>` and it's nested resources will represent various bit portions of it.
For example:
```
/sys_stat<bitfield<u8>> {
  addr: 0x00;
  default: 0x00;
  
  /cc_ready<bit<7>> {
    description: "Indicates that a fresh coulomb counter reading is available.";
    values: [
      0 => "0 = Fresh CC reading not yet available or bit is cleared by host microcontroller.",
      1 => "1 = Fresh CC reading is available. Remains latched high until cleared by host."
    ]
  }
}
```

Bit range is defined by using `bits` type: `/scd_t<bits<2, 0>> {}`.

Several similar bits or bit ranges can be defined by using resource arrays.
Shifts are possible by using `i` variable that goes from 0 to the size of the array-1:
```
/cellbal<bitfield<u16>> {
  addr: 0x02;
  description: "Balancing control for Cells 6-10";
  
  /cb<[bit<i + 6>; 5]> {
    values: [
      0 => "Cell balancing on Cell [i+6] is enabled",
      1 => "Cell balancing on Cell [i+6] is disabled"
    ]
  }
}
```

### Resource borrowing
Shared access to a mutable resource can lead to data races. To address this issue, resource can be borrowed by
a connected client or user code (or loaded wasm module).
```
/channels<[Cell<_>; max 8]> {
  /value<u8> {}
}
```
Borrowed resources can still be read, since all writes are 'atomic' in the sense that it is not possible to
perform partial writes like in memory.

### Resource conditional enable
`#cfg[name]` can be used before resource declaration to allow enabling/disabling it.
User code can decide whether to do it in run time or just once during initialization.

### Functions
Functions or methods can be defined as follows:
```
/start<fn()> {}
```

### Streams


### Access type
Resources with a type which is not a function or a stream are read-write by default (properties).
Other access types are:
* `rw` - default, but can be explicitly used anyway
* `ro` - read only
* `wo` - write only
* `const` - read only and guaranteed not to change

### Alias resources
```
/main {
  /x<u8> {}
  /y<alias<u16, _>> {
    bound: #../x * 10
  }
}
```

* `#/` points to `/main`
* `#./` points to child resources
* `#../` points to parent resources

### Built-in types and functions
#### Types
* `[T; numbound?]` - array of things, can be of fixed size (`numbound` = unsigned literal), bounded (`..4`, `2..8`) or unbounded.
* `Cell<T>` - borrowable resource, described above

#### Functions
* `indexof( [resource path] )` - returns type with enough bits to hold the index for specified resource array
  * `/array_of_resources<[_; 7]> {}`
  * `indexof(#/array_of_resources) == autonum<0..7>`
* `sizeof( [resource path] )` - returns resource array size or bound
  * `/channels<[_; max 12]> {}`
  * `sizeof(#/channels) == numbound<min 0 max 12>` not known during vhL compilation

### Resource naming
Use lower_snake_case.

## 4. Automatic UI building / `ui`
Flutter based application + supporting Dart libraries.

Features:
* Support for bytecode loading in order to extract types and api information
* Support for vhL source loading from external sources and compiling to bytecode (through Rust lib FFI or backend service)
* Provide input and output widgets for various types (number with SI support as spinner / dial / slide, string, color, ...)
* Generate documentation like UI with the ability to interact with server code
* Generate server mockup UI with ability to respond with user input, prerecorded answers or examples
* vhL syntax highlighting

## 5. Repository / `repo`
Public repository where versioned vhL sources can be uploaded to be used as dependencies.
Located in [vhrdtech/vhl-repo]()


## 6. Language Library / `langlib`
Language implementation itself, bundling together all of its components. To be used by `cli`, `ui` and other
project as a Rust crate dependency.

### Components
* **parser** - in charge of lexing and parsing source code into AST data structures
* **types** - Type system
* **serdes** - SerDes format and everything related to it
* **si** - SI datastructures, dimensional analysis
* **xpi** - API datastructures, checker
* **evolution** - SemVer checking machinery
* **dep** - loads dependencies from the repository, publishes new versions,
  can also load dependencies from git or local folder for development
* **ir** - Intermediate representation - generation, optimisation passes, export to human-readable format, used by
  codegen
* **codegen** - target code generation, for now implemented in Rust, not in vhL
  * **rust**
  * **dart**
  * **bytecode**
  * **cxx**
* **doc** - Documentation generator
* **patch** - codegen patch extraction and application (allows user to hand-fix codegen errors and shortcomings very
  quickly

## 7. CLI utility / `cli`
Utility for creating vhL projects, generating code, documentation, publishing to the repository, etc.
Works on top of language library (`src/lib.rs`).

## 8. / `doc`
Generate rich and useful documentation with navigation and search capabilities.

## 9. / `flow`
Describe how api calls is to be converted to and from messages and how messages are
flowing through data processing blocs up to the medium boundary (wires, radio, fiber, etc.)

Data processing blocks examples:
* protocol (TCP, UDP, WebSocket, UAVCAN, CANOpen, ...)
* compressor and decompressor
* scrambler and descrambler
* coder and decoder
* multiplexer and demultiplexer
* frame synchroniser (stream to frame converter) and frame to stream
* interface (Ethernet, CAN, USB, etc)

Features:
* Generate code that is interconnecting aforementioned blocks to produce a
working communication system.

## 10. / `interop`
Support for interoperability with other protocols and systems.

## 11. / `bootstrap`
Full-fledged language that can be used to express all the above features by itself.