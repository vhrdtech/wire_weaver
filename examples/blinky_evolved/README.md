# blinky_evolved

> Client of the evolved blinky API, run against devices of either version

Depends on `blinky_api_evolved` (the blinky API with a method added since the original `blinky_api`) and
`wire_weaver_client`. Shows that a client built against the evolved API still talks to a device running the
original one, and to one running the evolved one, without a special case on either side.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide, in particular how an API
evolves without breaking old firmware or old clients.
