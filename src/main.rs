#![allow(clippy::precedence)]

use anyhow::bail;
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, StreamConfig};
use fundsp::hacker::*;
use fundsp::prelude::AudioUnit64;
use midi_msg::{ChannelVoiceMsg, MidiMsg};
use midir::{Ignore, MidiInput, MidiInputPort};
use read_input::prelude::*;

#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

// Globally defined string parameters.
// Update these to modify what string is played, or to "tune" the existing string.
static TENSION: f64 = 48.86; // B string tension (N)
static LINEAR_DENSITY: f64 = 0.000477; // B string linear density (Kg/m)
static STRING_LENGTH: f64 = 0.64; // string length (meters)

// Main call that runs when program starts
fn main() -> anyhow::Result<()> {
    let mut midi_in = MidiInput::new("midir reading input")?;
    let in_port = get_midi_device(&mut midi_in)?;

    // set up shared variables
    let pitch = shared(0.0);
    let volume = shared(0.0);
    let pitch_bend = shared(1.0);
    let control = shared(0.0);

    // initialize output
    run_output(
        pitch.clone(),
        volume.clone(),
        pitch_bend.clone(),
        control.clone(),
    );

    // initialize midi input (non-blocking)
    run_input(midi_in, in_port, pitch, volume, pitch_bend, control)
}

/// (Partially from fundsp/examples/live_adsr.rs)
/// This function is where the `adsr_live()` function is employed. The `shared()` objects are wrapped
/// in `var()` objects in order to be placed in the signal graph.
/// * The `adsr_live()` modulates the volume of the sound over time. Play around with the different
///   values to get a feel for the impact of different ADSR levels. The `control` `shared()` is set
///   to 1.0 to start the attack and 0.0 to start the release.
/// * Then, we modulate the volume further using the MIDI velocity.
/// * Note that the `pitch` and `pitch_bend` parameters are not used currently.
///   Additional work would need to be done in order to integrate midi, as currently the
///   frequency produced is determined by a waveguide (delay) of fixed timing, and this would need to
///   be updated when a note changes or a new guitar string would need to be spawned.
fn create_sound(
    pitch: Shared<f64>,
    volume: Shared<f64>,
    pitch_bend: Shared<f64>,
    control: Shared<f64>,
) -> Box<dyn AudioUnit64> {
    // (experimental) get pitch from midi, handle div by zero errors
    // let mut freq = var(&pitch).value();
    // if freq == 0.0 {
    //     freq = freq + 1.0;
    // }

    // compute effective waveguide length
    let velocity = sqrt(TENSION / LINEAR_DENSITY);
    let waveguide_length = 2.0 * STRING_LENGTH / velocity;
    let waveguide = delay(waveguide_length);

    // generate impulse
    let impulse = dc(1.0)
        * var(&volume)
        * (var(&control) >> adsr_live(waveguide_length / 2., waveguide_length / 2., 0.0, 0.0));

    // options for feedback gain - each time the sample passes through it gets multipled by 0.995
    let feedback_gain = mul(0.995);

    // generate feedback with a delay loop
    let string_feedback = feedback2(waveguide, feedback_gain);

    // pluck the string by passing the impulse into the delay loop
    let pluck = impulse >> string_feedback;

    // generate resonant harmonics by filtering impulse
    let harmonic_q = 10.0;
    let root_freq_hz = waveguide_length.powi(-1);

    // // these should be feedbacks instead, but we need to generate an impulse, not constant tone
    let harmonic_2 = pluck.clone() >> bandpass_hz(root_freq_hz * 2.0, harmonic_q) * 1.0;
    let harmonic_3 = pluck.clone() >> bandpass_hz(root_freq_hz * 3., harmonic_q) * 0.5;
    let harmonic_4 = pluck.clone() >> bandpass_hz(root_freq_hz * 4., harmonic_q) * 0.5;
    let harmonic_5 = pluck.clone() >> bandpass_hz(root_freq_hz * 5., harmonic_q) * 0.3;
    let harmonic_6 = pluck.clone() >> bandpass_hz(root_freq_hz * 6., harmonic_q) * 0.2;

    // chain signals together into path
    let sound = pluck + harmonic_2 + harmonic_3 + harmonic_4 + harmonic_5 + harmonic_6;

    // (experimental) limiting, dc control, and declicking for safety
    // let mut sound = sound >> (declick() | declick()) >> (dcblock() | dcblock());
    // let mut sound = sound >> limiter_stereo((0.5, 1.0)); // comment to disable limiter (helpful for envelope testing)
    Box::new(sound)
}

// (From fundsp/examples/live_adsr.rs)
// Gets midi devices from host
fn get_midi_device(midi_in: &mut MidiInput) -> anyhow::Result<MidiInputPort> {
    midi_in.ignore(Ignore::None);
    let in_ports = midi_in.ports();
    if in_ports.is_empty() {
        bail!("No MIDI devices attached")
    } else {
        println!(
            "Chose MIDI device {}",
            midi_in.port_name(&in_ports[0]).unwrap()
        );
        Ok(in_ports[0].clone())
    }
}

/// (From fundsp/examples/live_adsr.rs)
/// This function is where MIDI events control the values of the `shared()` objects.
/// * A `NoteOn` event alters all four `shared()` objects:
///   * Using `midi_hz()`, a MIDI pitch is converted to a frequency and stored.
///   * MIDI velocity values range from 0 to 127. We divide by 127 and store in `volume`.
///   * Setting `pitch_bend` to 1.0 makes the bend neutral.
///   * Setting `control` to 1.0 starts the attack.
/// * A `NoteOff` event sets `control` to 0.0 to start the release.
/// * A `PitchBend` event calls `pitch_bend_factor()` to convert the MIDI values into
///   a scaling factor for the pitch, which it stores in `pitch_bend`.
fn run_input(
    midi_in: MidiInput,
    in_port: MidiInputPort,
    pitch: Shared<f64>,
    volume: Shared<f64>,
    pitch_bend: Shared<f64>,
    control: Shared<f64>,
) -> anyhow::Result<()> {
    println!("\nOpening connection");
    let in_port_name = midi_in.port_name(&in_port)?;
    let _conn_in = midi_in
        .connect(
            &in_port,
            "midir-read-input",
            move |_stamp, message, _| {
                let (msg, _len) = MidiMsg::from_midi(message).unwrap();
                if let MidiMsg::ChannelVoice { channel: _, msg } = msg {
                    println!("Received {msg:?}");
                    match msg {
                        ChannelVoiceMsg::NoteOn { note, velocity } => {
                            pitch.set_value(midi_hz(note as f64));
                            volume.set_value(velocity as f64 / 127.0);
                            pitch_bend.set_value(1.0);
                            control.set_value(1.0);
                        }
                        ChannelVoiceMsg::NoteOff { note, velocity: _ } => {
                            if pitch.value() == midi_hz(note as f64) {
                                control.set_value(-1.0);
                            }
                        }
                        ChannelVoiceMsg::PitchBend { bend } => {
                            pitch_bend.set_value(pitch_bend_factor(bend));
                        }
                        _ => {}
                    }
                }
            },
            (),
        )
        .unwrap();
    println!("Connection open, reading input from '{in_port_name}'");

    let _ = input::<String>().msg("(press enter to exit)...\n").get();
    println!("Closing connection");
    Ok(())
}

// (From fundsp/examples/live_adsr.rs)
// This function figures out the sample format and calls `run_synth()` accordingly.
fn run_output(
    pitch: Shared<f64>,
    volume: Shared<f64>,
    pitch_bend: Shared<f64>,
    control: Shared<f64>,
) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    let config = device.default_output_config().unwrap();
    match config.sample_format() {
        SampleFormat::F32 => {
            run_synth::<f32>(pitch, volume, pitch_bend, control, device, config.into())
        }
        SampleFormat::I16 => {
            run_synth::<i16>(pitch, volume, pitch_bend, control, device, config.into())
        }
        SampleFormat::U16 => {
            run_synth::<u16>(pitch, volume, pitch_bend, control, device, config.into())
        }
        _ => panic!("Unsupported format"),
    }
}

/// (From fundsp/examples/live_adsr.rs)
/// This function is where the sound is created and played. Once the sound is playing, it loops
/// infinitely, allowing the `shared()` objects to shape the sound in response to MIDI events.
fn run_synth<T: SizedSample + FromSample<f64>>(
    pitch: Shared<f64>,
    volume: Shared<f64>,
    pitch_bend: Shared<f64>,
    control: Shared<f64>,
    device: Device,
    config: StreamConfig,
) {
    std::thread::spawn(move || {
        let sample_rate = config.sample_rate.0 as f64;
        let mut sound = create_sound(pitch, volume, pitch_bend, control);
        sound.reset(Some(sample_rate));

        let mut next_value = move || sound.get_stereo();
        let channels = config.channels as usize;
        let err_fn = |err| eprintln!("an error occurred on stream: {err}");
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    write_data(data, channels, &mut next_value)
                },
                err_fn,
                None,
            )
            .unwrap();

        stream.play().unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
}

/// (From fundsp/examples/live_adsr.rs)
/// Algorithm is from here: https://sites.uci.edu/camp2014/2014/04/30/managing-midi-pitchbend-messages/
/// Converts MIDI pitch-bend message to +/- 1 semitone.
fn pitch_bend_factor(bend: u16) -> f64 {
    2.0_f64.powf(((bend as f64 - 8192.0) / 8192.0) / 12.0)
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> (f64, f64))
where
    T: SizedSample + FromSample<f64>,
{
    for frame in output.chunks_mut(channels) {
        let sample = next_sample();
        let left = T::from_sample(sample.0);
        let right: T = T::from_sample(sample.1);

        for (channel, sample) in frame.iter_mut().enumerate() {
            if channel & 1 == 0 {
                *sample = left;
            } else {
                *sample = right;
            }
        }
    }
}
