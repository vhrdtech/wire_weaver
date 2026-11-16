"""Read a logring card, ring file, image or split file: `open()`, tracks, recordings, markers, `Track.to_numpy()` and
`Track.to_pandas()`. The same API as the `logring` wheel, compiled into this one (logring's `logring_py`).

```python
from wire_weaver import logring

log = logring.open("recording.wlr")
log.track("sensor_0").to_pandas()
```
"""

from ._wire_weaver import logring as _logring

LogFile = _logring.LogFile
Track = _logring.Track
open = _logring.open
__version__ = _logring.__version__

__all__ = ["LogFile", "Track", "open", "__version__"]
