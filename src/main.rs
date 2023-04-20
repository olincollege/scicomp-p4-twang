use anyhow;
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use fundsp::hacker::*;

#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

fn main() {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("Failed to find a default output device");
    let config = device.default_output_config().unwrap();

    run::<f64>(&device, &config.into()).unwrap();
}

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f64>,
{
    let sample_rate = config.sample_rate.0 as f64;
    let channels = 1;
    let length = 4.0;
    let waveguide: An<Delay<f64>> = delay(length);

    let feedback_loop = feedback(waveguide >> lowpass_hz(1000.0, 1.0));

    let c = noise() >> feedback_loop;

    let mut c = c >> declick() >> dcblock() >> limiter(1.0);

    c.reset(Some(sample_rate));
    c.allocate();

    let mut next_value = move || assert_no_alloc(|| c.get_mono());

    let err_fn = |err| eprintln!("an error occured on stream: {}", err);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(50000));

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f64)
where
    T: SizedSample + FromSample<f64>,
{
    for frame in output.chunks_mut(channels) {
        let sample = next_sample();
        let mono = T::from_sample(sample);

        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = mono;
        }
    }
}
