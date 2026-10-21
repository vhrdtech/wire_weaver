use stm32_dfu_host::sim::SimStm32Dfu;
use stm32_dfu_host::{Dfuse, Error, FlashOptions, Image, Stage};

const FLASH: u32 = 0x0800_0000;
const SECTOR: u32 = 128 * 1024;
const UID: [u32; 3] = [0x0100_0000, 0xABCD_0000, 0x0000_0002];

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect()
}

#[tokio::test]
async fn reads_the_layout() {
    let sim = SimStm32Dfu::h72x(UID);
    let dfu = Dfuse::new(sim.clone()).unwrap();
    assert_eq!(dfu.layout().name, "Internal Flash");
    assert_eq!(dfu.layout().regions[0].sectors, 8);
    assert_eq!(dfu.transfer_size(), 1024);
    assert_eq!(sim.serial(), "01000002ABCD");
}

#[tokio::test]
async fn flash_erases_only_what_it_touches_and_verifies() {
    let sim = SimStm32Dfu::h72x(UID);
    // something in sector 3 that must survive, something old in sector 0 that must go
    sim.preload(FLASH + 3 * SECTOR, &[0x11; 64]);
    sim.preload(FLASH, &[0x00; 64]);
    let mut dfu = Dfuse::new(sim.clone()).unwrap();
    // bootloader-like image: 300 bytes at the start, 5 bytes (padded to a flash word) in sector 1
    let mut image = Image::from_bin(FLASH, data(300, 1)).unwrap();
    image.add(FLASH + SECTOR + 8, &[1, 2, 3, 4, 5]).unwrap();
    let mut stages = vec![];
    dfu.flash(&image, &FlashOptions::default(), &mut |s: Stage, done, total| {
        if done == total {
            stages.push(s);
        }
    })
    .await
    .unwrap();
    assert_eq!(stages, [Stage::Erase, Stage::Write, Stage::Verify]);
    assert_eq!(sim.erased(), [FLASH, FLASH + SECTOR]);
    assert_eq!(sim.memory(FLASH, 300), data(300, 1));
    assert_eq!(sim.memory(FLASH + SECTOR + 8, 5), [1, 2, 3, 4, 5]);
    assert_eq!(sim.memory(FLASH + 3 * SECTOR, 64), [0x11; 64]);
    // padded with the erased value
    assert_eq!(sim.memory(FLASH + SECTOR + 13, 19), [0xFF; 19]);
}

#[tokio::test]
async fn image_larger_than_a_transfer_and_a_sector() {
    let sim = SimStm32Dfu::h72x(UID);
    let mut dfu = Dfuse::new(sim.clone()).unwrap();
    let image = Image::from_bin(FLASH + SECTOR, data(SECTOR as usize + 5000, 9)).unwrap();
    dfu.flash(&image, &FlashOptions::default(), &mut ()).await.unwrap();
    assert_eq!(sim.erased(), [FLASH + SECTOR, FLASH + 2 * SECTOR]);
    assert_eq!(sim.memory(FLASH + SECTOR, SECTOR as usize + 5000), data(SECTOR as usize + 5000, 9));
}

#[tokio::test]
async fn verify_catches_a_bad_write() {
    let sim = SimStm32Dfu::h72x(UID);
    sim.corrupt_on_write(FLASH + 100);
    let mut dfu = Dfuse::new(sim).unwrap();
    let image = Image::from_bin(FLASH, data(200, 3)).unwrap();
    let err = dfu.flash(&image, &FlashOptions::default(), &mut ()).await.unwrap_err();
    assert!(matches!(err, Error::Verify { addr, .. } if addr == FLASH + 100), "{err}");
}

#[tokio::test]
async fn outside_memory_is_refused_before_anything_is_erased() {
    let sim = SimStm32Dfu::h72x(UID);
    let mut dfu = Dfuse::new(sim.clone()).unwrap();
    let image = Image::from_bin(0x2000_0000, vec![1; 64]).unwrap();
    let err = dfu.flash(&image, &FlashOptions::default(), &mut ()).await.unwrap_err();
    assert!(matches!(err, Error::OutOfMemory { addr: 0x2000_0000, .. }), "{err}");
    assert!(sim.erased().is_empty());
}

#[tokio::test]
async fn device_errors_are_reported_and_cleared() {
    let sim = SimStm32Dfu::h72x(UID);
    let mut dfu = Dfuse::new(sim).unwrap();
    let err = dfu.erase_sector(0x0900_0000).await.unwrap_err();
    assert!(matches!(err, Error::Device { what: "erase", .. }), "{err}");
    // usable again
    dfu.erase_sector(FLASH).await.unwrap();
    // writing without erasing: the sector is not empty any more
    dfu.write(FLASH, &[0u8; 32], &mut |_| {}).await.unwrap();
    let err = dfu.write(FLASH, &[0xAAu8; 32], &mut |_| {}).await.unwrap_err();
    assert!(matches!(err, Error::Device { what: "write", .. }), "{err}");
}

#[tokio::test]
async fn a_device_left_in_error_state_is_recovered_by_flash() {
    let sim = SimStm32Dfu::h72x(UID);
    let mut dfu = Dfuse::new(sim.clone()).unwrap();
    _ = dfu.erase_sector(0x0900_0000).await;
    let image = Image::from_bin(FLASH, data(64, 5)).unwrap();
    dfu.flash(&image, &FlashOptions::default(), &mut ()).await.unwrap();
    assert_eq!(sim.memory(FLASH, 64), data(64, 5));
}

#[tokio::test]
async fn leave_jumps_to_the_address() {
    let sim = SimStm32Dfu::h72x(UID);
    let mut dfu = Dfuse::new(sim.clone()).unwrap();
    assert_eq!(sim.left(), None);
    dfu.leave(FLASH).await.unwrap();
    assert_eq!(sim.left(), Some(FLASH));
}
