```text
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: AB(Copy), product: "sim-dfu", geometry_id: 4242, app_version: Some(1),
      version_floor: 1, manifest_digest: Some(1dd95effd1e00214), boot_state: Confirmed,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  01 80 00 00 73 69 6d 2d 64 66 …
→ stage.limits()
         4 B  02 03 00 02
← { max_piece: 64, in_flight: 1, live: true, stored: false, plan_area: 0, writes_in_background: true }
        13 B  02 80 40 00 00 00 01 80 00 00 …
→ stage.begin(target: Live, header: [35 B: 57 57 44 50 01 92 …])
        41 B  03 03 10 00 57 57 44 50 01 92 …
← Ok({ steps: 11, resumed: false, resume_from: 0 })
        13 B  03 80 80 0b 00 00 00 00 00 00 …
→ stage.erase(index: 0, block: 0)
        10 B  04 03 20 00 00 00 00 00 00 02
← Ok(())
         3 B  04 80 80
→ stage.erase(index: 1, block: 1)
        10 B  05 03 20 01 00 00 00 01 00 02
← Ok(())
         3 B  05 80 80
→ stage.erase(index: 2, block: 2)
        10 B  06 03 20 02 00 00 00 02 00 02
← Ok(())
         3 B  06 80 80
→ stage.erase(index: 3, block: 3)
        10 B  07 03 20 03 00 00 00 03 00 02
← Ok(())
         3 B  07 80 80
→ stage.write(index: 4, dst: 0, hash: 2780790d97efcff7, offset: 0, data: [64 B: 46 57 02 00 00 00 …])
        88 B  08 03 40 04 00 00 00 00 00 27 …
← Ok(())
         3 B  08 80 80
→ stage.copy(index: 5, dst: 1, src: 1, hash: 3ce293643207b87c)
        20 B  09 03 30 05 00 00 00 01 00 01 …
← Ok(())
         3 B  09 80 80
→ stage.write(index: 6, dst: 2, hash: edcc83d5b3974a90, offset: 0, data: [64 B: 61 70 70 20 76 32 …])
        88 B  0a 03 40 06 00 00 00 02 00 ed …
← Ok(())
         3 B  0a 80 80
→ stage.copy(index: 7, dst: 3, src: 5, hash: 1ec70fb257d1befc)
        20 B  0b 03 30 07 00 00 00 03 00 05 …
← Ok(())
         3 B  0b 80 80
→ stage.copy(index: 8, dst: 4, src: 4, hash: a809c61b42942960)
        20 B  0c 03 30 08 00 00 00 04 00 04 …
← Ok(())
         3 B  0c 80 80
→ stage.write(index: 9, dst: 5, hash: 3bf9a7bd49c0f887, offset: 0, data: [64 B: 6e 65 74 20 76 32 …])
        88 B  0d 03 40 09 00 00 00 05 00 3b …
← Ok(())
         3 B  0d 80 80
→ stage.write(index: 10, dst: 6, hash: d18f7ce680781257, offset: 0, data: [64 B: 6f 74 61 94 9a 86 …])
        88 B  0e 03 40 0a 00 00 00 06 00 d1 …
← Ok(())
         3 B  0e 80 80
   ── host resends step 5 as if its reply was lost: skipped
→ stage.copy(index: 5, dst: 1, src: 1, hash: 3ce293643207b87c)
        20 B  0f 03 30 05 00 00 00 01 00 01 …
← Ok(())
         3 B  0f 80 80
→ stage.finish()
         4 B  10 03 60 02
← Ok(Staged)
         3 B  10 80 82
→ boot.reboot(into: App)
         5 B  11 01 30 00 02
← Ok(())
         3 B  11 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── bootloader switches to the staged image: swaps the slots' contents page by page
   ── device runs the app
   ── host reconnects to the same device
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: AB(Copy), product: "sim-dfu", geometry_id: 4242, app_version: Some(2),
      version_floor: 1, manifest_digest: Some(5673030c91c1ef0d), boot_state: Trial,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  01 80 00 00 73 69 6d 2d 64 66 …
→ boot.confirm()
         4 B  02 01 10 02
← Ok(())
         3 B  02 80 80
→ node.node()
         4 B  03 00 00 02
← { role: App, profile: AB(Copy), product: "sim-dfu", geometry_id: 4242, app_version: Some(2),
      version_floor: 1, manifest_digest: Some(5673030c91c1ef0d), boot_state: Confirmed,
      last_result: { outcome: Confirmed, version: 2 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  03 80 00 00 73 69 6d 2d 64 66 …
```

<details markdown="1">
<summary>Wire bytes: 45 USB packets (full speed, 64 B max) (link setup and pings left out)</summary>

```text
   ── device runs the app
→    5 B  04 01 00 00 02
←   44 B  08 2a 01 80 00 00 73 69 6d 2d 64 66 75 92 10 00
          00 80 01 00 00 00 01 00 00 00 80 1d d9 5e ff d1
          e0 02 14 00 00 00 00 00 00 f0 01 57
→    5 B  04 02 03 00 02
←   15 B  08 0d 02 80 40 00 00 00 01 80 00 00 00 00 80
→   43 B  08 29 03 03 10 00 57 57 44 50 01 92 10 00 00 02
          00 00 00 0b 00 67 01 00 00 86 ba 52 71 00 2c c3
          5d c1 92 3c 26 b1 01 e5 86 3c 02
←   15 B  08 0d 03 80 80 0b 00 00 00 00 00 00 00 00 19
→   12 B  08 0a 04 03 20 00 00 00 00 00 00 02
←    4 B  03 04 80 80
→   12 B  08 0a 05 03 20 01 00 00 00 01 00 02
←    4 B  03 05 80 80
→   12 B  08 0a 06 03 20 02 00 00 00 02 00 02
←    4 B  03 06 80 80
→   12 B  08 0a 07 03 20 03 00 00 00 03 00 02
←    4 B  03 07 80 80
→   64 B  48 58 08 03 40 04 00 00 00 00 00 27 80 79 0d 97
          ef cf f7 00 00 00 00 46 57 02 00 00 00 2f 86 5d
          24 dc d1 cf 67 47 9e 75 3c f4 e9 e7 7f 5f b6 8d
          54 0c 01 ff 97 77 ce a5 6c 24 19 17 af 8f e6 bd
→   30 B  c8 1a 84 3c 31 2f c7 a7 fe d5 9c 54 49 47 df bf
          16 ed b4 6c 61 5f f7 d7 2e 00 89 02 f1 74
←    4 B  03 08 80 80
→   22 B  08 14 09 03 30 05 00 00 00 01 00 01 00 3c e2 93
          64 32 07 b8 7c 02
←    4 B  03 09 80 80
→   64 B  48 58 0a 03 40 06 00 00 00 02 00 ed cc 83 d5 b3
          97 4a 90 00 00 00 00 61 70 70 20 76 32 11 db 28
          16 4c de 9f 28 29 f3 40 2e 64 f6 b7 40 41 0b 58
          46 7c 0e cf 58 59 23 70 5e 94 26 e7 70 71 3b 88
→   30 B  c8 1a 76 ac 3e ff 88 89 53 a0 8e c4 56 17 a0 a1
          6b b8 a6 dc 6e 2f b8 b9 83 00 89 02 1e 16
←    4 B  03 0a 80 80
→   22 B  08 14 0b 03 30 07 00 00 00 03 00 05 00 1e c7 0f
          b2 57 d1 be fc 02
←    4 B  03 0b 80 80
→   22 B  08 14 0c 03 30 08 00 00 00 04 00 04 00 a8 09 c6
          1b 42 94 29 60 02
←    4 B  03 0c 80 80
→   64 B  48 58 0d 03 40 09 00 00 00 05 00 3b f9 a7 bd 49
          c0 f8 87 00 00 00 00 6e 65 74 20 76 32 87 6d 7c
          5d 3c 65 75 24 9f 85 94 75 54 7d 8d 3c b7 9d ac
          8d 6c 95 a5 54 cf b5 c4 a5 84 ad bd 6c e7 cd dc
→   30 B  c8 1a bd 9c c5 d5 84 ff e5 f4 d5 b4 dd ed 9c 17
          fd 0c ed cc f5 05 b4 2f 15 00 89 02 e6 79
←    4 B  03 0d 80 80
→   64 B  48 58 0e 03 40 0a 00 00 00 06 00 d1 8f 7c e6 80
          78 12 57 00 00 00 00 6f 74 61 94 9a 86 af 33 c8
          a6 ea ac b2 9e c7 4b e0 be 02 c4 ca b6 df 63 f8
          d6 1a dc e2 ce f7 7b 10 ee 32 f4 fa e6 0f 93 28
→   30 B  c8 1a 06 4a 0c 12 fe 27 ab 40 1e 62 24 2a 16 3f
          c3 58 36 7a 3c 42 2e 57 db 00 89 02 87 5e
←    4 B  03 0e 80 80
   ── host resends step 5 as if its reply was lost: skipped
→   22 B  08 14 0f 03 30 05 00 00 00 01 00 01 00 3c e2 93
          64 32 07 b8 7c 02
←    4 B  03 0f 80 80
→    5 B  04 10 03 60 02
←    4 B  03 10 80 82
→    6 B  05 11 01 30 00 02
←    4 B  03 11 80 80
←    3 B  30 c1 02
   ── device resets (into the app)
   ── bootloader switches to the staged image: swaps the slots' contents page by page
   ── device runs the app
   ── host reconnects to the same device
→    5 B  04 01 00 00 02
←   44 B  08 2a 01 80 00 00 73 69 6d 2d 64 66 75 92 10 00
          00 80 02 00 00 00 01 00 00 00 80 56 73 03 0c 91
          c1 ef 0d 30 00 00 00 00 00 f0 01 57
→    5 B  04 02 01 10 02
←    4 B  03 02 80 80
→    5 B  04 03 00 00 02
←   44 B  08 2a 03 80 00 00 73 69 6d 2d 64 66 75 92 10 00
          00 80 02 00 00 00 01 00 00 00 80 56 73 03 0c 91
          c1 ef 0d 00 10 02 00 00 00 f0 01 57
```

</details>
