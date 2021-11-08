## Very Hard Language / vhL
vhL is a language aimed at fully defining machine-to-machine communications. [TODO: write short and meaningful description]

vhL is not a regular language, it is (mostly) not designed to be executed, but rather it precisely describes what needs to be done in another languages.
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
The goal is to develop a rich type system with first class support for bounded numbers and SI units that will be able to represent even the most complex systems, while still being able to serialise everything in all the supported targets.

**Types:**
* Boolean (true / false)
* Discrete numbers:
  * Signed: `i8` / `i16` / `i32` / `i64` / `i128`
  * Signed with configurable length: `i<N>`, N = 1, 2, 3...
  * Unsigned: `u8`, `u16`, `u32`, `u64`, `u128`
  * Unsigned with configurable length: `u<N>`, N = 1, 2, 3...
* Fixed point numbers (Q notation):
  * Signed: `q3.12`
  * Unsigned: `uq1.15`
  * Implicitly scaled: `q12<u8>` 0..=255 * 2^-12
  * Full syntax to allow constant's to be easily used: `q<m, n>`, `uq<m, n>`
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

Generic struct with related units, `rate` unit must be `U`'s unit divided by Seconds.
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

### 3. / `api`
### (4). / `ui`
### 5. / `doc`
### 6. / `flow`
### 7. / `interop`
### 8. / `bootstrap`
