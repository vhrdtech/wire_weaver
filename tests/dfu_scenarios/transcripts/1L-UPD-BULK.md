```text
   ── device runs the app
   ── link up: dfu_lean_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: InPlace, product: "sim-dfu", geometry_id: 4242, app_version: Some(1), version_floor: 1,
      manifest_digest: Some(1dd95effd1e00214), boot_state: Confirmed, last_result: { outcome: Nothing,
      version: 0 }, offers: { boot_control: true, inventory: false, stage: false, stage_bulk: false } }
        41 B  01 80 01 73 69 6d 2d 64 66 75 …
→ boot.reboot(into: Updater)
         5 B  02 01 30 10 02
← Ok(())
         3 B  02 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the updater)
   ── device runs the updater
   ── link up: dfu_updater_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: Bootloader, profile: InPlace, product: "sim-dfu", geometry_id: 4242, app_version: Some(1),
      version_floor: 1, manifest_digest: Some(1dd95effd1e00214), boot_state: Confirmed,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        41 B  01 80 11 73 69 6d 2d 64 66 75 …
→ stage.limits()
         4 B  02 03 00 02
← { max_piece: 32, in_flight: 1, live: true, stored: false, plan_area: 0, writes_in_background: false }
        13 B  02 80 20 00 00 00 01 80 00 00 …
→ bulk.begin_bulk(target: Live, header: [35 B: 57 57 44 50 01 92 …], total_len: 484, ack_every: 0)
        49 B  03 04 00 00 57 57 44 50 01 92 …
← Ok({ steps: 10, resumed: false, resume_from: 0 })
        13 B  03 80 80 0a 00 00 00 00 00 00 …
→ bulk.plan_ack: StreamOpen { resume: None, max_age_us: None, args: tail(0:[]) }
         5 B  04 04 28 00 02
← ()
         6 B  04 80 00 00 00 00
→ bulk.plan: open
         4 B  05 04 16 02
← bulk.plan: credit: have 0 bytes, may send 2062 more
         7 B  00 83 41 50 e9 84 02
← written
         2 B  05 81
→ bulk.plan ⇐ { offset: 0, data: [32 B: 01 00 00 03 00 00 …] } (sink position 0)
        42 B  00 04 14 00 00 00 00 00 01 00 …
→ bulk.plan ⇐ { offset: 32, data: [32 B: 75 3c f4 e9 e7 7f …] } (sink position 37)
        42 B  00 04 14 25 20 00 00 00 75 3c …
→ bulk.plan ⇐ { offset: 64, data: [32 B: d5 9c 54 49 47 df …] } (sink position 74)
        42 B  00 04 14 4a 40 00 00 00 d5 9c …
→ bulk.plan ⇐ { offset: 96, data: [32 B: d3 da 10 e3 4c 3b …] } (sink position 111)
        42 B  00 04 14 6f 60 00 00 00 d3 da …
→ bulk.plan ⇐ { offset: 128, data: [32 B: 33 3a 70 43 ac 9b …] } (sink position 148)
        43 B  00 04 14 81 14 80 00 00 00 33 …
→ bulk.plan ⇐ { offset: 160, data: [32 B: 03 02 00 ed cc 83 …] } (sink position 185)
        43 B  00 04 14 81 39 a0 00 00 00 03 …
→ bulk.plan ⇐ { offset: 192, data: [32 B: f6 b7 40 41 0b 58 …] } (sink position 222)
        43 B  00 04 14 81 5e c0 00 00 00 f6 …
→ bulk.plan ⇐ { offset: 224, data: [32 B: 56 17 a0 a1 6b b8 …] } (sink position 259)
        43 B  00 04 14 82 03 e0 00 00 00 56 …
→ bulk.plan ⇐ { offset: 256, data: [32 B: a8 09 c6 1b 42 94 …] } (sink position 296)
        43 B  00 04 14 82 28 00 01 00 00 a8 …
→ bulk.plan ⇐ { offset: 288, data: [32 B: e6 e8 1b 3a cb 36 …] } (sink position 333)
        43 B  00 04 14 82 4d 20 01 00 00 e6 …
→ bulk.plan ⇐ { offset: 320, data: [32 B: 46 48 7b 9a 2b 96 …] } (sink position 370)
        43 B  00 04 14 82 72 40 01 00 00 46 …
→ bulk.plan ⇐ { offset: 352, data: [32 B: 5d 3c 65 75 24 9f …] } (sink position 407)
        43 B  00 04 14 83 17 60 01 00 00 5d …
→ bulk.plan ⇐ { offset: 384, data: [32 B: bd 9c c5 d5 84 ff …] } (sink position 444)
        43 B  00 04 14 83 3c 80 01 00 00 bd …
→ bulk.plan ⇐ { offset: 416, data: [32 B: 12 57 40 00 6f 74 …] } (sink position 481)
        43 B  00 04 14 83 61 a0 01 00 00 12 …
→ bulk.plan ⇐ { offset: 448, data: [32 B: e2 ce f7 7b 10 ee …] } (sink position 518)
        43 B  00 04 14 84 06 c0 01 00 00 e2 …
→ bulk.plan ⇐ { offset: 480, data: [4 B: 42 2e 57 db] } (sink position 555)
        15 B  00 04 14 84 2b e0 01 00 00 42 …
← bulk.plan_ack ⇒ { have: 484, status: Complete }
         9 B  00 82 42 e4 01 00 00 20 02
→ bulk.plan: credit query: sent 564 bytes
         6 B  06 04 16 6c e8 12
← bulk.plan: credit: have 564 bytes, may send 2062 more
         8 B  06 83 41 5c e8 1e 98 42
→ stage.finish()
         4 B  07 03 60 02
← Ok(Confirmed)
         3 B  07 80 80
→ boot.reboot(into: App)
         5 B  08 01 30 00 02
← Ok(())
         3 B  08 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── device runs the app
   ── link up: dfu_lean_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: InPlace, product: "sim-dfu", geometry_id: 4242, app_version: Some(2), version_floor: 1,
      manifest_digest: Some(5673030c91c1ef0d), boot_state: Confirmed, last_result: { outcome: Confirmed,
      version: 2 }, offers: { boot_control: true, inventory: false, stage: false, stage_bulk: false } }
        41 B  01 80 01 73 69 6d 2d 64 66 75 …
```

<details markdown="1">
<summary>Wire bytes: 42 WebSocket frames (host frames unmasked, mask key zeroed) (link setup and pings left out)</summary>

```text
   ── device runs the app
→   11 B  82 85 00 00 00 00 00 01 00 00 02
←   44 B  82 2a 00 01 80 01 73 69 6d 2d 64 66 75 92 10 00
          00 80 01 00 00 00 01 00 00 00 80 1d d9 5e ff d1
          e0 02 14 00 00 00 00 00 00 80 01 57
→   12 B  82 86 00 00 00 00 00 02 01 30 10 02
←    6 B  82 04 00 02 80 80
←    4 B  82 02 0c 02
   ── device resets (into the updater)
   ── device runs the updater
→   11 B  82 85 00 00 00 00 00 01 00 00 02
←   44 B  82 2a 00 01 80 11 73 69 6d 2d 64 66 75 92 10 00
          00 80 01 00 00 00 01 00 00 00 80 1d d9 5e ff d1
          e0 02 14 00 00 00 00 00 00 f0 01 57
→   11 B  82 85 00 00 00 00 00 02 03 00 02
←   16 B  82 0e 00 02 80 20 00 00 00 01 80 00 00 00 00 00
→   56 B  82 b2 00 00 00 00 00 03 04 00 00 57 57 44 50 01
          92 10 00 00 02 00 00 00 0a 00 e4 01 00 00 86 ba
          52 71 00 2c c3 5d 10 9b 6b 17 9f ae b3 bd e4 01
          00 00 00 00 00 00 3c 02
←   16 B  82 0e 00 03 80 80 0a 00 00 00 00 00 00 00 00 19
→   12 B  82 86 00 00 00 00 00 04 04 28 00 02
←    9 B  82 07 00 04 80 00 00 00 00
→   11 B  82 85 00 00 00 00 00 05 04 16 02
←   10 B  82 08 00 00 83 41 50 e9 84 02
←    5 B  82 03 00 05 81
→   49 B  82 ab 00 00 00 00 00 00 04 14 00 00 00 00 00 01
          00 00 03 00 00 27 80 79 0d 97 ef cf f7 40 00 46
          57 02 00 00 00 2f 86 5d 24 dc d1 cf 67 47 9e 0c
          02
→   49 B  82 ab 00 00 00 00 00 00 04 14 25 20 00 00 00 75
          3c f4 e9 e7 7f 5f b6 8d 54 0c 01 ff 97 77 ce a5
          6c 24 19 17 af 8f e6 bd 84 3c 31 2f c7 a7 fe 0c
          02
→   49 B  82 ab 00 00 00 00 00 00 04 14 4a 40 00 00 00 d5
          9c 54 49 47 df bf 16 ed b4 6c 61 5f f7 d7 2e 03
          01 00 3c e2 93 64 32 07 b8 7c 40 00 68 61 6c 0c
          02
→   49 B  82 ab 00 00 00 00 00 00 04 14 6f 60 00 00 00 d3
          da 10 e3 4c 3b 40 72 eb f2 28 fb 64 53 58 8a 03
          0a 40 13 7c 6b 70 a2 1b 22 58 2b 94 83 88 ba 0c
          02
→   50 B  82 ac 00 00 00 00 00 00 04 14 81 14 80 00 00 00
          33 3a 70 43 ac 9b a0 d2 4b 52 88 5b c4 b3 b8 ea
          63 6a a0 73 dc cb d0 02 7b 82 b8 8b f4 01 01 00
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 81 39 a0 00 00 00
          03 02 00 ed cc 83 d5 b3 97 4a 90 40 00 61 70 70
          20 76 32 11 db 28 16 4c de 9f 28 29 f3 40 2e 64
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 81 5e c0 00 00 00
          f6 b7 40 41 0b 58 46 7c 0e cf 58 59 23 70 5e 94
          26 e7 70 71 3b 88 76 ac 3e ff 88 89 53 a0 8e c4
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 82 03 e0 00 00 00
          56 17 a0 a1 6b b8 a6 dc 6e 2f b8 b9 83 02 03 00
          05 00 1e c7 0f b2 57 d1 be fc 01 02 00 03 04 00
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 82 28 00 01 00 00
          a8 09 c6 1b 42 94 29 60 40 00 75 69 83 ee 6e 08
          b6 b8 eb 0a 9b 06 86 20 ce d0 03 22 b3 1e 9e 38
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 82 4d 20 01 00 00
          e6 e8 1b 3a cb 36 b6 50 fe 00 33 52 e3 4e ce 68
          16 18 4b 6a fb 66 e6 80 2e 30 63 82 13 7e fe 98
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 82 72 40 01 00 00
          46 48 7b 9a 2b 96 16 b0 5e 60 03 05 00 3b f9 a7
          bd 49 c0 f8 87 40 00 6e 65 74 20 76 32 87 6d 7c
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 83 17 60 01 00 00
          5d 3c 65 75 24 9f 85 94 75 54 7d 8d 3c b7 9d ac
          8d 6c 95 a5 54 cf b5 c4 a5 84 ad bd 6c e7 cd dc
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 83 3c 80 01 00 00
          bd 9c c5 d5 84 ff e5 f4 d5 b4 dd ed 9c 17 fd 0c
          ed cc f5 05 b4 2f 15 03 06 00 d1 8f 7c e6 80 78
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 83 61 a0 01 00 00
          12 57 40 00 6f 74 61 94 9a 86 af 33 c8 a6 ea ac
          b2 9e c7 4b e0 be 02 c4 ca b6 df 63 f8 d6 1a dc
          0c 02
→   50 B  82 ac 00 00 00 00 00 00 04 14 84 06 c0 01 00 00
          e2 ce f7 7b 10 ee 32 f4 fa e6 0f 93 28 06 4a 0c
          12 fe 27 ab 40 1e 62 24 2a 16 3f c3 58 36 7a 3c
          0c 02
→   22 B  82 90 00 00 00 00 00 00 04 14 84 2b e0 01 00 00
          42 2e 57 db 04 02
→   13 B  82 87 00 00 00 00 00 06 04 16 6c e8 12
←   12 B  82 0a 00 00 82 42 e4 01 00 00 20 02
←   11 B  82 09 00 06 83 41 5c e8 1e 98 42
→   11 B  82 85 00 00 00 00 00 07 03 60 02
←    6 B  82 04 00 07 80 80
→   12 B  82 86 00 00 00 00 00 08 01 30 00 02
←    6 B  82 04 00 08 80 80
←    4 B  82 02 0c 02
   ── device resets (into the app)
   ── device runs the app
→   11 B  82 85 00 00 00 00 00 01 00 00 02
←   44 B  82 2a 00 01 80 01 73 69 6d 2d 64 66 75 92 10 00
          00 80 02 00 00 00 01 00 00 00 80 56 73 03 0c 91
          c1 ef 0d 00 10 02 00 00 00 80 01 57
```

</details>
