# 1. Core features

## String support

Done

Vec<T> support Done
Option support Prototype
Vec<u8> special handling Done
enum discriminant type selection Done
Evolution rules checker 3 Not started
USB link + ww_client_server integration tests Prototype
u4 support on user side Done
#[final_evolution] support Prototype
AST & conversion from syn Done
Plain enums Done
Data enums Done
Structs support Done
Map support Backlog
Tuples support - might work, check Prototype
Fixed size array support 70 Done
LEB numbers support 65 Backlog
ClientServer API calls Done
ClientServer streams Prototype
Format of self, derive ShrinkWrap / introspect Done
Server no_std Done
Client std Done
AST processor Prototype
use support Prototype
Dependencies from git Backlog
Result and Option support Done
Result and Option in Vec Done
UI for API Backlog
Value to bytes in runtime / runtime literals Backlog
u12, uN support Prototype
Evolution report gen 60 Backlog
USB Vec recycling Done
Document things 2 Stalled
Move to nusb Done
Register description registry? 45 Backlog
Support unit Prototype
model streams as functions which one can call to change parameters, subscribe, stop, etc? Backlog
match on slices? [0] [1] ... [0, 0] [0, 1] …, leads to multi-read/write support 4 Not started
#[max_size] bound, together with String and array bounds 8 Not started
Fuzz ShrinkWrap 2 Not started
shrink wrap Vec flavor 3 Not started
test speed of ser and des on MCU and host Perf Not started
measure how much flash does it all take with various functions used and not? Perf Not started
auto calculate buffer sizes for event, arguments and output serdes 45 Backlog
report git dirty flag in protocol version Done
try nusb queue after measuring Done
try queue on slow VM, does it lead to less or no missed packets? Perf Backlog
add bulk with lower prio / USB Done
add user handler functions right into rx loop? Backlog
change match order to process common requests faster 47 Not started
more custom and user friendly syntax, namely nested levels, no macro calls, potential SI support, etc Backlog
inplace editing capabilities 49 Not started
enum FutureVersion(discriminant, bytes) / FutureVersion(discriminant)        1 Not started
last vec in struct without size? 50 Backlog
Option and Result - take two id numbers, for flag and itself Backlog
ShrinkWrap for data format serdes needs (DB, file storage)                Prototype
impl Trait support - accessing APIs via standard traits Prototype
#[repr(un)]                Done
#[derive(ShrinkWrap)] → attr, generate Owned variant automatically Done
“Explain” encoding, after eval is ready, record operations done on buffers so that each bit can be traced 60 Not started
derive_shrink_wrap act on whole mod at once? or read from file with own syntax if it turns out to be incompatible with
Rust lexer Backlog
Replace all types with just Sized and Unsized, let traits to the rest? will need RefStr and probably RefVecU8 unless
partial specialization is available Done
allowing evolving any type + #[evolve_all] - all struct and enums are Unsized by default Done
Size analysis, including maximum buffer usage in worst case 46 Not started
Global type registry, id inclue wire_weaver version, type version and hash of type fields? Prototype
Example/template Prototype
Advanced example/template 5 Not started
Go through TODO items #1 1 Not started
Check that tuple variants are evolvable Not started
Vec and Result cannot be added? Not started
Keep reconnecting demo with streams and ui feedback Not started
RefVec builder 10 Not started
Dynamic UI tool MVP 2 Prototype
Step by step in-place builder / partial builder 10 Not started
ww_gpio higher level API using trait only Prototype
Generate missing methods via CLI tool 3 Not started
Properties errors Done
User errors Done
Try to remove root() from client Done
Try to return preparation object (PrepareCall) in client instead of many similar methods Done
Introspect as a trait, remove from ww_client_server Won't do
Generate doc comments for all resource types 20 Not started
Generate and publish docs for ww_stdlib crates? 30 Not started
#[cfg(feature = “”)] in enums - bad idea, very hard to handle with dynamic serdes Won't do
external types in api crates, pub use or full path? - ww_self ast generates full path Done
Dynamic value serdes 2 Stalled
impl same trait several times → enum selector 10 Stalled
index chain array → named arguments 1 Stalled
Multi-dimensional arrays (2D, 3D)        12 Not started
Methods args to user struct 20 Not started
Handle eob properly, evolve 19 Not started
Direction adjustment 0 Not started
impl trait source/path from Cargo 10 Not started
Somehow detect local use and/or dirty git, add CRC of API and types to version → implemented with SHA signature Done
Refactor u4 → nibble Done
CRC of resources, strict mode 50 Not started
Manual signature/SHA calculation Not started
Multi read/write/call 4 Not started
Return streams from functions 15 Not started
Built-in array size getter → range or list Done
Compress ww_self strings with pre-shared dictionary? Not started
Return Result or WwResult from methods 5 Not started
Return Result or WwResult from methods 5 Not started
USB protocol 10 Prototype
Ethernet protocol 10 Stalled
CAN protocol server 10 Not started
CAN protocol client 11 Not started
Ethernet protocol client Not started
USB NCM protocol 10 Not started
UART protocol Not started
Internet protocol / WireGuard Not started
FFI example Not started
Setup Python in examples Not started
🌐 Web tool like pest 5 Not started
Prefix enum discriminants Dev Backlog
EtherCAT protocol 10 Not started