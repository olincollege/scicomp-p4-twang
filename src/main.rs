use fundsp::hacker::*;

const SAMPLE_RATE: f64 = 44100.0;

fn main() {
    let length = 4.0;
    let waveguide: An<Delay<f64>> = delay(length);

    let feedback_loop = feedback(waveguide >> lowpass_hz(1000.0, 1.0));
}
