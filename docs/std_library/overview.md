# Standard library types overview

* ww_numeric
    * `NumericValue`: value of the supported numeric types
    * `NumericBaseType`: discrete, floating and fixed point number types
    * `NumericAnyType`: base types + subtype and shift-scale
* ww_si - SI and derived values using `NumericValue` as storage
* ww_date_time
    * `DateTime`: ISO 8601 combined date and time with optional time zone and optional nanoseconds.
      Minimum size is 32 bits.
    * `NaiveDate`: ISO 8601 calendar date without timezone. Year stored as shifted by 2025, minimum size is 13 bits.
    * `NaiveTime`: ISO 8601 time without timezone. Size is 18 bits without nanoseconds and 49 bits with nanoseconds.
* ww_version
    * `Version`: SemVer version (including pre and build strings), no alloc
    * `VersionOwned`: SemVer version, same as `Version` but uses String's
    * `CompactVersion`: Global type id + major and minor version numbers, uses UNib32 for all three
* ww_client_server - `Request`, `RequestKind`, `Event`, `EventKind`, `Error` used for client-server API model.
* ww_can_bus - CAN Bus types and API
* ww_dfu - Firmware update API
* ww_log_bare_metal - Logging types and API for no_std bare metal targets
* ww_self - WireWeaver of WireWeaver itself for dynamic access to APIs, expression eval and introspection.

