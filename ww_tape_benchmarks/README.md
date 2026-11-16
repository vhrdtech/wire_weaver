# ww_tape_benchmarks

Benchmarks of `ww_tape`. Its binary runs every float sample codec on simulated, decoded and sensor data next to the same quantities stored as fixed-point integers with `DELTA`, checks every value comes back bit for bit, and prints a Markdown table of ratio, bits per value and encode and decode time per value.

```sh
just bench-sample-codecs              # fetches the speech file once, then runs
just bench-sample-codecs --chunk 1024 --repeats 9
```

The data sets, how they are made and the measured table are in the docs: [Sample codecs](../docs/transport/tap.md#sample-codecs).

The `summaries` binary measures series summaries: MB/s of sample bytes per element type through the RAW codec alone, `SeriesSummaries::push` with every statistic, and both, with the CPU's vector extensions and the load average.

```sh
just bench-summaries
just bench-summaries --frames 1024 --mib 64
```
