## Very Hard Language / vhL
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

### 1. Advanced type system / `types`
The goal is to develop a rich type system with first class support for bounded numbers and SI units that will be
able to represent even the most complex systems, while still being able to serialise everything in all
the supported targets.

Features:
* Bytecode target should be supported
* Provide `Default`, `Example` and `Allowed` values for any user defined type

**Types:**
* Boolean (true / false)
* Discrete numbers:
  * Signed: `i8` / `i16` / `i32` / `i64` / `i128`
  * Signed with configurable length: `i{expr -> u32}`
  * Unsigned: `u8`, `u16`, `u32`, `u64`, `u128`
  * Unsigned with configurable length: `u{expr -> u32}`
* Fixed point numbers (Q notation):
  * Signed: `q3.12`
  * Unsigned: `uq1.15`
  * Implicitly scaled: `q12<u8>` 0..=255 * 2^-12 (m=0, n=12)
  * Full syntax to allow constant's to be easily used: `q{expr -> (u32, u32)}`, `uq{expr -> (u32, u32)}`
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

**Bounded / checked numbers:**

Simple checked numbers where only a range of values is allowed:
* `u16 in 1..=512` - values 513 and above are not allowed
* `u<N>` or `i<N>` - valid values from -2^(N-1) to 2^(N-1)-1
* ? auto-bound-number: `-10..21` to let vhL choose the smallest representation possible automatically
* fixed point can be derived as well: `-1.0,-0.9..1.0`

Set of allowed values:
* `u8 in {0..=8, 12, 16, 20, 24, 32, 48, 64}`

Number classes/traits?:
* Natural: 1, 2, 3, 4, 5, ...
* Whole: 0, 1, 2, 3, 4, 5, ...
* Integer: ..., -3, -2, -1, 0, 1, 2, 3, ...
* ?Fraction: a/b, a and b are Whole, b != 0
* ? Rational: p/q, p and q are Integer, q != 0
* Real: Rational + Irrational
* Complex

Possible syntax:
* `u32: Natural` ?
* `Natural<u32>` ?
* `u16: 1..=512` - to be in sync with traits?
* `u16<1..=512>`, `u16<Natural>`, `u16<0..=8, 12, 16>` - make numbers kinda like higher kind?

**SI support:**

Specify any SI unit for any number, define derived and custom units, define unit relations.
* `current: q3.12 [A]`
* `velocity: f32 [m/s]`

Custom units with optional shortened name:
```
unit Byte = [1, "B"];

unit Ki = [1024];
unit Mi = [1024 * Ki];
unit Gi = [1024 * Mi];

unit Frame = [1];
unit Packet = [1];
```

Generic struct with related units, `rate` unit must be `U`'s unit divided by Seconds:
```
struct RateCounter<T: WholeNumber[U]> {
    current: T[U], 
    rate: f32[U/s]
}

struct ByteCounters {
  ingress: RateCounter<u64[Byte]>,
  egress: RateCounter<u64[Byte]>,
}

struct FrameCounters {
  ingress: RateCounter<u64[Frame]>,
  egress: RateCounter<u64[Frame]>,
}
```

### 2. Serialisation and Deserialisation / `serdes`
Platform independent, efficient binary format. Support for no_std environment without memory allocator.

* Generate serialisation and deserialisation code in various languages (Rust, Dart, C++, ...).
* Ability to configure and tweak the code generator
* For now codegen to be implemented in Rust, maybe later by the language itself
* Support/utils libraries in target languages written manually
* Various cli tool commands

Format features:
* Cross-platform
* No type information, only data and necessary service information
* Bounds checking
* Optional sanity checks (cannot cover all errors though, use other means of checking for faulty channels,
like CRC or FEC).
* Proper padding for efficient processing?
* Backwards compatibility, add new fields to the end
* Little endian

### 3. / `api`
Define hardware device or software service API/ABI as a collection primitives such as methods, properties,
streams and more.

Features:
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

### 4. Automatic UI building / `ui`
Flutter based application + supporting Dart libraries.

Features:
* Support for bytecode loading in order to extract types and api information
* Support for vhL source loading from external sources and compiling to bytecode (through Rust lib FFI or backend service)
* Provide input and output widgets for various types (number with SI support as spinner / dial / slide, string, color, ...)
* Generate documentation like UI with the ability to interact with server code
* Generate server mockup UI with ability to respond with user input, prerecorded answers or examples
* vhL syntax highlighting

### 5. Repository / `repo`
Public repository where versioned vhL sources can be uploaded to be used as dependencies.
Located in [vhrdtech/vhl-repo]()


### 6. Language Library / `langlib`
Language implementation itself, bundling together all of its components. To be used by `cli`, `ui` and other
project as a Rust crate dependency.

Components:
* **parser** - in charge of lexing and parsing source code into AST data structures
* **types** - Type system
* **serdes** - SerDes format and everything related to it
* **si** - SI datastructures, dimensional analysis
* **api** - API datastructures, checker
* **versioning** - SemVer checking machinery
* **dep** - loads dependencies from the repository, publishes new versions,
can also load dependencies from git or local folder for development
* **ir** - Intermediate representation - generation, optimisation passes, export to human-readable format,  used by codegen
* **codegen** - target code generation, for now implemented in Rust, not in vhL
  * **rust**
  * **dart**
  * **bytecode**
  * **cxx**
* **doc** - Documentation generator
* **patch** - codegen patch extraction and application (allows user to hand-fix codegen errors and shortcomings very quickly

### 7. CLI utility / `cli`
Utility for creating vhL projects, generating code, documentation, publishing to the repository, etc.
Works on top of language library (`src/lib.rs`).

### 8. / `doc`
Generate rich and useful documentation with navigation and search capabilities.

### 9. / `flow`
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

### 10. / `interop`
Support for interoperability with other protocols and systems.

### 11. / `bootstrap`
Full-fledged language that can be used to express all the above features by itself.