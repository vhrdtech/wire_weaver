# customary

Tests the Mars Climate Orbiter case (STD-72) end to end: a device reports thruster impulse in pound-force seconds, a navigation client wants newton seconds.

Checks both the generated client's typed conversion and the dynamic client's conversion from introspection data alone; the value converts exactly or is refused, never silently reinterpreted.
