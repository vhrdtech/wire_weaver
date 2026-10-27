```text
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         5 B  01 40 00 00 02
← { role: App, profile: AB(Copy), product: "sim-dfu", geometry_id: 4242, product_id: 1, seal: Unsigned,
      app_version: Some(1), version_floor: 1, manifest_digest: Some(1dd95effd1e00214), boot_state: Confirmed,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        46 B  01 80 00 00 73 69 6d 2d 64 66 …
→ stage.limits()
         5 B  02 40 30 00 02
← { max_piece: 64, in_flight: 1, live: true, stored: false, plan_area: 0, writes_in_background: true }
        13 B  02 80 40 00 00 00 01 80 00 00 …
→ bulk.begin_bulk(target: Live, header: [35 B: 57 57 44 50 01 92 …], total_len: 359, ack_every: 0)
        50 B  03 40 40 00 00 57 57 44 50 01 …
← Ok({ steps: 11, resumed: false, resume_from: 0 })
        13 B  03 80 80 0b 00 00 00 00 00 00 …
→ bulk.plan: open its session at 0
         8 B  04 40 41 88 01 00 00 02
← bulk.plan: ack: have 0 units, may send 2062 more bytes
         8 B  00 83 41 50 00 90 0e 02
← bulk.plan: opened at 0, buffer of 2062 bytes
         9 B  04 80 01 00 00 40 90 0e 00
→ bulk.plan ⇐ { offset: 0, data: [64 B: 01 00 00 01 01 00 …] } (sink position 0, 1 unit)
        76 B  00 40 41 a0 00 00 00 00 00 01 …
→ bulk.plan ⇐ { offset: 64, data: [64 B: e6 bd 84 3c 31 2f …] } (sink position 1, 1 unit)
        76 B  00 40 41 a0 01 40 00 00 00 e6 …
→ bulk.plan ⇐ { offset: 128, data: [64 B: 28 29 f3 40 2e 64 …] } (sink position 2, 1 unit)
        76 B  00 40 41 a0 02 80 00 00 00 28 …
→ bulk.plan: query: sent 3 units
         7 B  05 40 41 67 00 03 02
← bulk.plan: ack: have 3 units, may send 2062 more bytes
         8 B  05 83 41 50 03 90 0e 02
   ── host restarts; the device keeps its session
→ Disconnect(RequestByUser)
         1 B  02
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ bulk.begin_bulk(target: Live, header: [35 B: 57 57 44 50 01 92 …], total_len: 359, ack_every: 0)
        50 B  01 40 40 00 00 57 57 44 50 01 …
← Ok({ steps: 11, resumed: true, resume_from: 192 })
        13 B  01 80 80 0b 00 00 00 80 c0 00 …
→ bulk.plan_ack: StreamOpen { resume: None, max_age_us: None, codecs: None, args: tail(0:[]) }
         5 B  02 40 42 80 02
← ()
         6 B  02 80 00 00 00 00
   ── the plan sink continues its session where the device is
→ bulk.plan: open its session at 3
         8 B  03 40 41 88 01 03 00 02
← bulk.plan: ack: have 3 units, may send 2062 more bytes
         8 B  00 83 41 50 03 90 0e 02
← bulk.plan: opened at 3, buffer of 2062 bytes
         9 B  03 80 01 03 03 40 90 0e 00
→ bulk.plan ⇐ { offset: 192, data: [64 B: 02 04 00 04 00 a8 …] } (sink position 3, 1 unit)
        76 B  00 40 41 a0 03 c0 00 00 00 02 …
→ bulk.plan ⇐ { offset: 256, data: [64 B: e7 cd dc bd 9c c5 …] } (sink position 4, 1 unit)
        76 B  00 40 41 a0 04 00 01 00 00 e7 …
→ bulk.plan ⇐ { offset: 320, data: [39 B: d6 1a dc e2 ce f7 …] } (sink position 5, 1 unit)
        50 B  00 40 41 a0 05 40 01 00 00 d6 …
← bulk.plan_ack ⇒ { have: 359, status: Complete }
         9 B  00 82 42 67 01 00 00 20 02
→ bulk.plan: query: sent 6 units
         7 B  04 40 41 67 00 06 02
← bulk.plan: ack: have 6 units, may send 2062 more bytes
         8 B  04 83 41 50 06 90 0e 02
→ stage.finish()
         5 B  05 40 36 00 02
← Ok(Staged)
         3 B  05 80 82
→ boot.reboot(into: App)
         6 B  06 40 13 00 00 02
← Ok(())
         3 B  06 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── bootloader switches to the staged image: swaps the slots' contents page by page
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ boot.confirm()
         5 B  01 40 11 00 02
← Ok(())
         3 B  01 80 80
→ node.node()
         5 B  02 40 00 00 02
← { role: App, profile: AB(Copy), product: "sim-dfu", geometry_id: 4242, product_id: 1, seal: Unsigned,
      app_version: Some(2), version_floor: 1, manifest_digest: Some(5673030c91c1ef0d), boot_state: Confirmed,
      last_result: { outcome: Confirmed, version: 2 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        46 B  02 80 00 00 73 69 6d 2d 64 66 …
```

<details markdown="1">
<summary>Wire bytes: 27 UDP datagrams (link setup and pings left out)</summary>

```text
   ── device runs the app
→   11 B  03 05 01 40 00 00 02 f1 44 33 b4
←   53 B  03 08 2e 01 80 00 00 73 69 6d 2d 64 66 75 92 10
          00 00 01 00 00 00 08 01 00 00 00 01 00 00 00 80
          1d d9 5e ff d1 e0 02 14 00 00 00 00 00 00 f0 01
          57 e8 8a 34 ed
→   11 B  04 05 02 40 30 00 02 3f e5 5f 12
←   20 B  04 08 0d 02 80 40 00 00 00 01 80 00 00 00 00 80
          06 e4 55 c3
→   57 B  05 08 32 03 40 40 00 00 57 57 44 50 01 92 10 00
          00 02 00 00 00 0b 00 67 01 00 00 86 ba 52 71 00
          2c c3 5d c1 92 3c 26 b1 01 e5 86 67 01 00 00 00
          00 00 00 3c 02 a9 a3 1f f3
←   20 B  05 08 0d 03 80 80 0b 00 00 00 00 00 00 00 00 19
          19 4f f6 d8
→   15 B  06 08 08 04 40 41 88 01 00 00 02 13 c6 5c f8
←   30 B  06 08 08 00 83 41 50 00 90 0e 02 7c 8b 4a e3 08
          09 04 80 01 00 00 40 90 0e 00 21 bb 1a 82
→  259 B  07 08 4c 00 40 41 a0 00 00 00 00 00 01 00 00 01
          01 00 01 02 00 01 03 00 03 00 00 27 80 79 0d 97
          ef cf f7 40 00 46 57 02 00 00 00 2f 86 5d 24 dc
          d1 cf 67 47 9e 75 3c f4 e9 e7 7f 5f b6 8d 54 0c
          01 ff 97 77 ce a5 6c 24 19 17 af 8f 00 89 02 f5
          10 45 fa 08 4c 00 40 41 a0 01 40 00 00 00 e6 bd
          84 3c 31 2f c7 a7 fe d5 9c 54 49 47 df bf 16 ed
          b4 6c 61 5f f7 d7 2e 02 01 00 01 00 3c e2 93 64
          32 07 b8 7c 03 02 00 ed cc 83 d5 b3 97 4a 90 40
          00 61 70 70 20 76 32 11 db 28 16 4c de 9f 00 89
          02 ea 1e 97 12 08 4c 00 40 41 a0 02 80 00 00 00
          28 29 f3 40 2e 64 f6 b7 40 41 0b 58 46 7c 0e cf
          58 59 23 70 5e 94 26 e7 70 71 3b 88 76 ac 3e ff
          88 89 53 a0 8e c4 56 17 a0 a1 6b b8 a6 dc 6e 2f
          b8 b9 83 02 03 00 05 00 1e c7 0f b2 57 d1 be fc
          00 89 02 ba e0 e7 ee 07 05 40 41 67 00 03 02 19
          d9 8c d3
←   15 B  07 08 08 05 83 41 50 03 90 0e 02 36 f5 91 78
   ── host restarts; the device keeps its session
→    8 B  08 30 c1 02 40 95 74 fc
→   57 B  03 08 32 01 40 40 00 00 57 57 44 50 01 92 10 00
          00 02 00 00 00 0b 00 67 01 00 00 86 ba 52 71 00
          2c c3 5d c1 92 3c 26 b1 01 e5 86 67 01 00 00 00
          00 00 00 3c 02 cd 31 ae fd
←   20 B  0b 08 0d 01 80 80 0b 00 00 00 80 c0 00 00 00 19
          fa f0 8d 6f
→   11 B  04 05 02 40 42 80 02 4a c1 a9 7e
←   12 B  0c 06 02 80 00 00 00 00 85 ee 73 ad
   ── the plan sink continues its session where the device is
→   15 B  05 08 08 03 40 41 88 01 03 00 02 52 17 3d 69
←   30 B  0d 08 08 00 83 41 50 03 90 0e 02 d6 a3 de d2 08
          09 03 80 01 03 03 40 90 0e 00 98 7a dd 56
→  233 B  06 08 4c 00 40 41 a0 03 c0 00 00 00 02 04 00 04
          00 a8 09 c6 1b 42 94 29 60 03 05 00 3b f9 a7 bd
          49 c0 f8 87 40 00 6e 65 74 20 76 32 87 6d 7c 5d
          3c 65 75 24 9f 85 94 75 54 7d 8d 3c b7 9d ac 8d
          6c 95 a5 54 cf b5 c4 a5 84 ad bd 6c 00 89 02 00
          a1 d3 84 08 4c 00 40 41 a0 04 00 01 00 00 e7 cd
          dc bd 9c c5 d5 84 ff e5 f4 d5 b4 dd ed 9c 17 fd
          0c ed cc f5 05 b4 2f 15 03 06 00 d1 8f 7c e6 80
          78 12 57 40 00 6f 74 61 94 9a 86 af 33 c8 a6 ea
          ac b2 9e c7 4b e0 be 02 c4 ca b6 df 63 f8 00 89
          02 3d 3b 20 69 08 32 00 40 41 a0 05 40 01 00 00
          d6 1a dc e2 ce f7 7b 10 ee 32 f4 fa e6 0f 93 28
          06 4a 0c 12 fe 27 ab 40 1e 62 24 2a 16 3f c3 58
          36 7a 3c 42 2e 57 db 7c 02 20 a4 eb ca 07 04 40
          41 67 00 06 02 ab 32 f7 1f
←   30 B  0e 08 09 00 82 42 67 01 00 00 20 02 d5 6a a3 45
          08 08 04 83 41 50 06 90 0e 02 1f 3b a8 f8
→   11 B  07 05 05 40 36 00 02 00 5f 1a 95
←    9 B  0f 03 05 80 82 8e 77 39 68
→   12 B  08 06 06 40 13 00 00 02 ef d3 1f f7
←   16 B  00 03 06 80 80 2a 3f 21 06 30 c1 02 af bd c0 39
   ── device resets (into the app)
   ── bootloader switches to the staged image: swaps the slots' contents page by page
   ── device runs the app
→   11 B  03 05 01 40 11 00 02 b6 8d d7 a9
←    9 B  03 03 01 80 80 7f 53 ce 44
→   11 B  04 05 02 40 00 00 02 af 00 34 36
←   53 B  04 08 2e 02 80 00 00 73 69 6d 2d 64 66 75 92 10
          00 00 01 00 00 00 08 02 00 00 00 01 00 00 00 80
          56 73 03 0c 91 c1 ef 0d 00 10 02 00 00 00 f0 01
          57 eb da 23 57
```

</details>
