//! A sensor node sends a reading over a simulated radio link, and the gateway reads it back: `cargo run -p
//! sensor_packet --example sensor_packet_radio`.

fn main() {
    print!("{}", sensor_packet::radio::transcript());
}
