# Twang!
String synthesis in Rust, by Aydin O'Leary (@zbwrm) and Jasper Katzban (@jasperkatzban). This is for the final, self-determined project in SP23's Scientific Computing, taught by Dr. Carrie Nugent.

Uses an adapted version of the Karplus-Strong model as described [here](https://www.ee.columbia.edu/~ronw/dsp/).

In addition to being based on the above paper, this project makes heavy use of the FunDSP crate and is largely patterned after their provided examples, especially `live_adsr.rs`.

## Usage
To use, compile and run with `cargo run`. The program looks for any available MIDI input, including external keyboards or software-defined MIDI pipelines.

Rust is typically installed and managed by `rustup`. The Rust Foundation's guide to installing Rust can be found [here](https://www.rust-lang.org/tools/install).
