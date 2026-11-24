//! The wasm slot firmware on B135B over USB (SRV-85): flash `stm32h725ig-wasm-slot` with both interpreters, then
//! `just hw-wasm-slot`. Each interpreter's slot: calls into its module, a reload with another API that the client
//! relearns for that slot only, a trapping module unloaded while the firmware goes on, and the numbers.
use std::time::Duration;
use wasm_slot::{Calc, CalcEvolved, WasmSlot};
use wasm_slot_api::{Image, Interpreter, LoadRefused};
use wire_weaver::ww_api::mount::MountPresenceOwned;
use wire_weaver_client::CallError;
use wire_weaver_client::MountView;

const BOTH: [Interpreter; 2] = [Interpreter::Wasefire, Interpreter::Wasmi];

async fn connect() -> WasmSlot {
    WasmSlot::config(|c| c)
        .connect()
        .await
        .expect("B135B with the wasm slot firmware on USB")
}

fn hash_of(presence: &MountPresenceOwned) -> Option<Vec<u8>> {
    match presence {
        MountPresenceOwned::Up { hash, .. } => Some(hash.no_docs.hash.clone()),
        _ => None,
    }
}

async fn view_of(
    slot: &WasmSlot,
    interpreter: Interpreter,
    line: &wire_weaver_client::ClientLine,
) -> MountView {
    match interpreter {
        Interpreter::Wasefire => slot.wasefire().unwrap().view_for(line).await.unwrap(),
        _ => slot.wasmi().unwrap().view_for(line).await.unwrap(),
    }
}

/// Waits until the view's slot is up with a model other than `not`.
async fn up_with_other(view: &MountView, not: Option<Vec<u8>>) -> Vec<u8> {
    let mut status = view.status();
    let up = tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|p| hash_of(p).is_some_and(|h| Some(&h) != not.as_ref())),
    )
    .await
    .expect("the slot comes up")
    .unwrap()
    .clone();
    hash_of(&up).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs B135B with the wasm slot firmware on USB"]
async fn calls_reload_and_trap() {
    let slot = connect().await;
    let own = slot.cmd.device_api_info();
    for interpreter in BOTH {
        slot.load(interpreter, Image::Calc).call().await.unwrap();
        let line = Calc::client_line();
        let view = view_of(&slot, interpreter, &line).await;
        let first = up_with_other(&view, None).await;
        let calc = Calc::on_mount(&view).unwrap();
        assert_eq!(calc.add(2, 3).call().await.unwrap(), 5);
        assert_eq!(calc.add(u32::MAX, 2).call().await.unwrap(), 1, "wraps");

        // the evolved module: the view relearns this slot, the firmware's own facts stay
        slot.load(interpreter, Image::CalcEvolved)
            .call()
            .await
            .unwrap();
        up_with_other(&view, Some(first)).await;
        assert_eq!(
            view.commander().device_api_info().user_api_version.crate_id,
            "wasm_module_api_evolved"
        );
        let evolved_line = CalcEvolved::client_line();
        let view = view_of(&slot, interpreter, &evolved_line).await;
        let evolved = CalcEvolved::on_mount(&view).unwrap();
        assert_eq!(evolved.mul(6, 7).call().await.unwrap(), 42);
        let after = slot.cmd.device_api_info();
        assert_eq!(after.user_api_hash.no_docs, own.user_api_hash.no_docs);
        assert_eq!(after.user_api_version, own.user_api_version);

        // a trap unloads the module, the firmware answers on
        let faults = slot.counters(interpreter).call().await.unwrap().faults;
        assert!(
            evolved.trap().call().await.is_err(),
            "a trap answers no value"
        );
        let counters = slot.counters(interpreter).call().await.unwrap();
        assert_eq!(counters.faults.get(), faults.get() + 1);
        let mut status = view.status();
        tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|p| hash_of(p).is_none()),
        )
        .await
        .expect("the slot empties")
        .unwrap();
    }
    for interpreter in BOTH {
        slot.unload(interpreter).call().await.unwrap();
    }
}

/// wasmi runs out of fuel in `spin`: the module is unloaded instead of hanging the firmware. wasefire-interpreter
/// has no fuel (SRV-86), so it isn't called there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs B135B with the wasm slot firmware on USB"]
async fn wasmi_stops_a_loop() {
    let slot = connect().await;
    slot.load(Interpreter::Wasmi, Image::Calc)
        .call()
        .await
        .unwrap();
    let line = Calc::client_line();
    let view = view_of(&slot, Interpreter::Wasmi, &line).await;
    up_with_other(&view, None).await;
    let calc = Calc::on_mount(&view).unwrap();
    let faults = slot
        .counters(Interpreter::Wasmi)
        .call()
        .await
        .unwrap()
        .faults;
    assert!(calc.spin().call().await.is_err());
    let counters = slot.counters(Interpreter::Wasmi).call().await.unwrap();
    assert_eq!(counters.faults.get(), faults.get() + 1);
    assert_eq!(slot.heap().call().await.unwrap().size.get(), 192 * 1024);
}

/// Prints what each interpreter built into the firmware took: heap, cycles and stack of a load and of a call (`add`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs B135B with the wasm slot firmware on USB"]
async fn numbers() {
    let slot = connect().await;
    for interpreter in BOTH {
        slot.unload(interpreter).call().await.unwrap();
    }
    for interpreter in BOTH {
        let before = slot.heap().call().await.unwrap().used.get();
        slot.reset_heap_peak().call().await.unwrap();
        match slot.load(interpreter, Image::Calc).call().await {
            Err(CallError::Device(LoadRefused::NotBuilt)) => {
                println!("{interpreter:?}: not in this firmware");
                continue;
            }
            loaded => loaded.unwrap(),
        }
        let heap = slot.heap().call().await.unwrap();
        let line = Calc::client_line();
        let view = view_of(&slot, interpreter, &line).await;
        up_with_other(&view, None).await;
        let calc = Calc::on_mount(&view).unwrap();
        let mut process = vec![];
        let mut forward = vec![];
        for i in 0..100 {
            assert_eq!(calc.add(i, 1).call().await.unwrap(), i + 1);
            let t = slot.timing(interpreter).call().await.unwrap();
            process.push(t.process.get());
            forward.push(t.forward.get());
        }
        process.sort();
        forward.sort();
        let t = slot.timing(interpreter).call().await.unwrap();
        let after_calls = slot.heap().call().await.unwrap();
        let mhz = t.cpu_clock.get() / 1_000_000;
        println!(
            "{interpreter:?} @ {mhz} MHz: instantiate {} cycles, {} B stack; slot load {} cycles, {} B stack; \
             heap +{} B (peak +{} B), after 100 calls +{} B (peak +{} B); \
             add: process min {} median {} max {} cycles, forward median {} cycles, {} B stack",
            t.instantiate.get(),
            t.instantiate_stack.get(),
            t.load.get(),
            t.load_stack.get(),
            heap.used.get() - before,
            heap.peak.get() - before,
            after_calls.used.get() - before,
            after_calls.peak.get() - before,
            process[0],
            process[50],
            process[99],
            forward[50],
            t.forward_stack.get(),
        );
        slot.unload(interpreter).call().await.unwrap();
    }
}
