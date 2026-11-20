# multi_root

Tests of two wire lines of one API in one crate (EVO-37): `multi_root_api::wire_v1::Thermo` at w1.2 and
`multi_root_api::wire_v2::Thermo` at w2.1. A server and a client of each line are generated here, each from its
root's module path; each device reports its own line in link setup and each client connects only to its own.
