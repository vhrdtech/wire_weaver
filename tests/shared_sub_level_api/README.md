# shared_sub_level_api

Two wire lines of one API in one crate that reach the same sub-traits (`common::Channel` and `common::Indication`,
standing in for a real indication trait) at different places in each line.

Used by `tests/shared_sub_level`, where one client of each trait serves both lines. See FEATURES.md EVO-37
(several-wire-lines-in-one-api-crate) and CLT-68.
