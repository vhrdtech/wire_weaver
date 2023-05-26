use rhai::{Engine, EvalAltResult};

pub fn main() -> Result<(), Box<EvalAltResult>> {
    let engine = Engine::new();
    let script = "40 + 2";
    // engine.run(script)?;
    let result = engine.eval::<i64>(script)?;
    println!("r: {result}");

    Ok(())
}
