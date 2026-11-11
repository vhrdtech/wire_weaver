```text
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         5 B  01 40 00 00 02
← { role: App, profile: InPlace, product: "sim-dfu", geometry: 4242, seal: Unsigned, app: Some({ product: 1,
      version: 1, security_counter: 1 }), security_floor: 1, manifest_digest: Some(1dd95effd1e00214),
      boot_state: Confirmed, last_result: { outcome: Nothing, version: None }, offers: { boot_control: true,
      inventory: true, stage: true, stage_bulk: true } }
        46 B  01 80 01 73 69 6d 2d 64 66 75 …
→ stage.limits()
         5 B  02 40 30 00 02
← { max_piece: 64, in_flight: 1, live: false, stored: true, plan_area: 1024, writes_in_background: true }
        11 B  02 80 40 00 01 40 00 04 00 00 …
→ bulk.begin_bulk(target: Stored, header: [35 B: 57 57 44 50 01 92 …], total_len: 484, ack_every: 0)
        50 B  03 40 40 00 10 57 57 44 50 01 …
← Ok({ steps: 10, resumed: false, resume_step: 0, resume_byte: 0 })
        17 B  03 80 80 0a 00 00 00 00 00 00 …
→ bulk.plan_ack: StreamOpen { resume: None, max_age: None, codecs: None, args: tail(0:[]) }
         5 B  04 40 42 80 02
← ()
         6 B  04 80 00 00 00 00
→ bulk.plan: open its session at 0
         8 B  05 40 41 88 01 00 00 02
← bulk.plan: ack: have 0 units, may send 2062 more bytes
         8 B  00 83 41 50 00 90 0e 02
← bulk.plan: opened at 0, buffer of 2062 bytes
         9 B  05 80 01 00 00 40 90 0e 00
→ bulk.plan ⇐ { offset: 0, data: [64 B: 01 00 00 03 00 00 …] } (sink position 0, 1 unit)
        76 B  00 40 41 a0 00 00 00 00 00 01 …
→ bulk.plan ⇐ { offset: 64, data: [64 B: d5 9c 54 49 47 df …] } (sink position 1, 1 unit)
        76 B  00 40 41 a0 01 40 00 00 00 d5 …
→ bulk.plan ⇐ { offset: 128, data: [64 B: 33 3a 70 43 ac 9b …] } (sink position 2, 1 unit)
        76 B  00 40 41 a0 02 80 00 00 00 33 …
→ bulk.plan ⇐ { offset: 192, data: [64 B: f6 b7 40 41 0b 58 …] } (sink position 3, 1 unit)
        76 B  00 40 41 a0 03 c0 00 00 00 f6 …
→ bulk.plan: query: sent 4 units
         7 B  06 40 41 67 00 04 02
← bulk.plan: ack: have 4 units, may send 2062 more bytes
         8 B  06 83 41 50 04 90 0e 02
   ── half way: the app keeps doing its job and answers anything else
→ node.node()
         5 B  07 40 00 00 02
← { role: App, profile: InPlace, product: "sim-dfu", geometry: 4242, seal: Unsigned, app: Some({ product: 1,
      version: 1, security_counter: 1 }), security_floor: 1, manifest_digest: Some(1dd95effd1e00214),
      boot_state: Confirmed, last_result: { outcome: Nothing, version: None }, offers: { boot_control: true,
      inventory: true, stage: true, stage_bulk: true } }
        46 B  07 80 01 73 69 6d 2d 64 66 75 …
→ bulk.plan: open its session at 0
         9 B  08 40 41 88 82 01 00 00 02
← bulk.plan: ack: have 0 units, may send 2062 more bytes
         8 B  00 83 41 50 00 90 0e 02
← bulk.plan: opened at 0, buffer of 2062 bytes
        10 B  08 80 82 01 00 00 40 90 0e 00
→ bulk.plan ⇐ { offset: 256, data: [64 B: a8 09 c6 1b 42 94 …] } (sink position 0, 1 unit)
        76 B  00 40 41 a0 00 00 01 00 00 a8 …
→ bulk.plan ⇐ { offset: 320, data: [64 B: 46 48 7b 9a 2b 96 …] } (sink position 1, 1 unit)
        76 B  00 40 41 a0 01 40 01 00 00 46 …
→ bulk.plan ⇐ { offset: 384, data: [64 B: bd 9c c5 d5 84 ff …] } (sink position 2, 1 unit)
        76 B  00 40 41 a0 02 80 01 00 00 bd …
→ bulk.plan ⇐ { offset: 448, data: [36 B: e2 ce f7 7b 10 ee …] } (sink position 3, 1 unit)
        47 B  00 40 41 a0 03 c0 01 00 00 e2 …
← bulk.plan_ack ⇒ { have: 484, status: Complete }
         9 B  00 82 42 e4 01 00 00 20 02
→ bulk.plan: query: sent 4 units
         7 B  09 40 41 67 00 04 02
← bulk.plan: ack: have 4 units, may send 2062 more bytes
         8 B  09 83 41 50 04 90 0e 02
→ stage.finish()
         5 B  0a 40 36 00 02
← Ok(PlanStored)
         3 B  0a 80 84
→ boot.reboot(into: App)
         6 B  0b 40 13 00 00 02
← Ok(())
         3 B  0b 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── bootloader applies the stored plan in place: 10 steps
   ── bootloader: image verified against the descriptor, plan area cleared
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         5 B  01 40 00 00 02
← { role: App, profile: InPlace, product: "sim-dfu", geometry: 4242, seal: Unsigned, app: Some({ product: 1,
      version: 2, security_counter: 2 }), security_floor: 1, manifest_digest: Some(5673030c91c1ef0d),
      boot_state: Confirmed, last_result: { outcome: Confirmed, version: Some(2) },
      offers: { boot_control: true, inventory: true, stage: true, stage_bulk: true } }
        50 B  01 80 01 73 69 6d 2d 64 66 75 …
```

<details markdown="1">
<summary>Wire bytes: 24 UDP datagrams (link setup and pings left out)</summary>

```text
   ── device runs the app
→   11 B  02 05 01 40 00 00 02 45 4f 44 12
←   53 B  04 08 2e 01 80 01 73 69 6d 2d 64 66 75 92 10 00
          00 08 01 00 00 00 01 00 00 00 01 00 00 00 01 00
          00 00 80 1d d9 5e ff d1 e0 02 14 00 00 f0 01 14
          97 c1 b9 81 bb
→   11 B  03 05 02 40 30 00 02 b1 db f8 d7
←   18 B  05 08 0b 02 80 40 00 01 40 00 04 00 00 80 a1 c6
          87 d5
→   57 B  04 08 32 03 40 40 00 10 57 57 44 50 01 92 10 00
          00 02 00 00 00 0a 00 e4 01 00 00 86 ba 52 71 00
          2c c3 5d 10 9b 6b 17 9f ae b3 bd e4 01 00 00 00
          00 00 00 3c 02 28 da 9b 8b
←   24 B  06 08 11 03 80 80 0a 00 00 00 00 00 00 00 00 00
          00 00 00 59 24 84 c6 c2
→   11 B  05 05 04 40 42 80 02 5e 3f 9e 57
←   12 B  07 06 04 80 00 00 00 00 ce 11 40 26
→   15 B  06 08 08 05 40 41 88 01 00 00 02 8d c6 f6 34
←   30 B  08 08 08 00 83 41 50 00 90 0e 02 7a a0 3c b1 08
          09 05 80 01 00 00 40 90 0e 00 7c 7c 50 7c
→  341 B  07 08 4c 00 40 41 a0 00 00 00 00 00 01 00 00 03
          00 00 27 80 79 0d 97 ef cf f7 40 00 46 57 02 00
          00 00 2f 86 5d 24 dc d1 cf 67 47 9e 75 3c f4 e9
          e7 7f 5f b6 8d 54 0c 01 ff 97 77 ce a5 6c 24 19
          17 af 8f e6 bd 84 3c 31 2f c7 a7 fe 00 89 02 c7
          7b c8 67 08 4c 00 40 41 a0 01 40 00 00 00 d5 9c
          54 49 47 df bf 16 ed b4 6c 61 5f f7 d7 2e 03 01
          00 3c e2 93 64 32 07 b8 7c 40 00 68 61 6c d3 da
          10 e3 4c 3b 40 72 eb f2 28 fb 64 53 58 8a 03 0a
          40 13 7c 6b 70 a2 1b 22 58 2b 94 83 88 ba 00 89
          02 03 b8 9c 30 08 4c 00 40 41 a0 02 80 00 00 00
          33 3a 70 43 ac 9b a0 d2 4b 52 88 5b c4 b3 b8 ea
          63 6a a0 73 dc cb d0 02 7b 82 b8 8b f4 01 01 00
          03 02 00 ed cc 83 d5 b3 97 4a 90 40 00 61 70 70
          20 76 32 11 db 28 16 4c de 9f 28 29 f3 40 2e 64
          00 89 02 1d 50 0c 7e 08 4c 00 40 41 a0 03 c0 00
          00 00 f6 b7 40 41 0b 58 46 7c 0e cf 58 59 23 70
          5e 94 26 e7 70 71 3b 88 76 ac 3e ff 88 89 53 a0
          8e c4 56 17 a0 a1 6b b8 a6 dc 6e 2f b8 b9 83 02
          03 00 05 00 1e c7 0f b2 57 d1 be fc 01 02 00 03
          04 00 00 89 02 22 34 70 0e 07 06 40 41 67 00 04
          02 43 55 25 ad
←   15 B  09 08 08 06 83 41 50 04 90 0e 02 6a e1 bf 39
   ── half way: the app keeps doing its job and answers anything else
→   11 B  08 05 07 40 00 00 02 ab e5 74 39
←   53 B  0a 08 2e 07 80 01 73 69 6d 2d 64 66 75 92 10 00
          00 08 01 00 00 00 01 00 00 00 01 00 00 00 01 00
          00 00 80 1d d9 5e ff d1 e0 02 14 00 00 f0 01 14
          97 1c 65 90 54
→   16 B  09 08 09 08 40 41 88 82 01 00 00 02 47 5b e4 bf
←   31 B  0b 08 08 00 83 41 50 00 90 0e 02 7b c6 de 28 08
          0a 08 80 82 01 00 00 40 90 0e 00 b0 d3 71 36
→  312 B  0a 08 4c 00 40 41 a0 00 00 01 00 00 a8 09 c6 1b
          42 94 29 60 40 00 75 69 83 ee 6e 08 b6 b8 eb 0a
          9b 06 86 20 ce d0 03 22 b3 1e 9e 38 e6 e8 1b 3a
          cb 36 b6 50 fe 00 33 52 e3 4e ce 68 16 18 4b 6a
          fb 66 e6 80 2e 30 63 82 13 7e fe 98 00 89 02 af
          7c 64 07 08 4c 00 40 41 a0 01 40 01 00 00 46 48
          7b 9a 2b 96 16 b0 5e 60 03 05 00 3b f9 a7 bd 49
          c0 f8 87 40 00 6e 65 74 20 76 32 87 6d 7c 5d 3c
          65 75 24 9f 85 94 75 54 7d 8d 3c b7 9d ac 8d 6c
          95 a5 54 cf b5 c4 a5 84 ad bd 6c e7 cd dc 00 89
          02 f5 19 df 0a 08 4c 00 40 41 a0 02 80 01 00 00
          bd 9c c5 d5 84 ff e5 f4 d5 b4 dd ed 9c 17 fd 0c
          ed cc f5 05 b4 2f 15 03 06 00 d1 8f 7c e6 80 78
          12 57 40 00 6f 74 61 94 9a 86 af 33 c8 a6 ea ac
          b2 9e c7 4b e0 be 02 c4 ca b6 df 63 f8 d6 1a dc
          00 89 02 88 aa 84 c7 08 2f 00 40 41 a0 03 c0 01
          00 00 e2 ce f7 7b 10 ee 32 f4 fa e6 0f 93 28 06
          4a 0c 12 fe 27 ab 40 1e 62 24 2a 16 3f c3 58 36
          7a 3c 42 2e 57 db 4c 02 5a fb a9 dc 07 09 40 41
          67 00 04 02 fd c2 22 aa
←   30 B  0c 08 09 00 82 42 e4 01 00 00 20 02 3c 92 bc 31
          08 08 09 83 41 50 04 90 0e 02 84 5f c8 91
→   11 B  0b 05 0a 40 36 00 02 a5 a2 ea d0
←    9 B  0d 03 0a 80 84 e6 c6 c6 f0
→   12 B  0c 06 0b 40 13 00 00 02 cb ce cb cf
←   16 B  0e 03 0b 80 80 18 12 c9 b1 30 c1 02 9c ca 1f d9
   ── device resets (into the app)
   ── bootloader applies the stored plan in place: 10 steps
   ── bootloader: image verified against the descriptor, plan area cleared
   ── device runs the app
→   11 B  02 05 01 40 00 00 02 45 4f 44 12
←   57 B  04 08 32 01 80 01 73 69 6d 2d 64 66 75 92 10 00
          00 08 01 00 00 00 02 00 00 00 02 00 00 00 01 00
          00 00 80 56 73 03 0c 91 c1 ef 0d 00 18 02 00 00
          00 f0 01 54 97 29 d7 66 02
```

</details>
