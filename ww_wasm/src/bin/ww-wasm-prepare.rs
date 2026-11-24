//! `ww-wasm-prepare IN OUT`: validates a module binary and writes it with wasefire-interpreter's side table in front,
//! the form `ww_wasm::wasefire::WasefireModule` loads (wasmi ignores the section).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: ww-wasm-prepare IN.wasm OUT.wasm");
        return ExitCode::FAILURE;
    };
    let binary = match std::fs::read(input) {
        Ok(binary) => binary,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let prepared = match wasefire_interpreter::prepare(&binary) {
        Ok(prepared) => prepared,
        Err(e) => {
            eprintln!("{input}: not a module wasefire-interpreter runs: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(output, &prepared) {
        eprintln!("{output}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
