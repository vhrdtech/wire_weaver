# quantities_api

`ww_si` quantities in an API, as dynamic clients see them (CLT-31): a named unit, a `Series` of `Timed`
values, a compound unit (`Per`), a value whose unit is only known at run time (`AnyQuantity`, STD-37), and
`Fixed` values (STD-55) with their unit, prefix and storage fixed at compile time.

Used by `tests/quantities`, never built on its own.
