# ww_quantity

`ww_si` quantities in dynamic clients: a value of `ww_si::Quantity<U>`, `AnyQuantity`, `Fixed`, `Point` or a coded unit known only from an API's introspection data, with its unit.

- `unit_of`, `fixed_of`, `coded_of`: the unit of a type in a prepared `ApiBundleOwned`.
- `DynQuantity`: a value with that unit, shown as stored (`3300 mV`) or scaled (`3.3 V`), converted to SI.
- `parse_quantity_in`, `parse_point_in`: text input in any unit that converts exactly.

The dynamic client of `wire_weaver_client`, the `ww` CLI, the Python bindings and `ww_json` use it. It depends only on `ww_self`, `ww_si`, `ww_numeric` and `shrink_wrap`, so a JSON or settings codec takes it without a transport.
