use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use tokio::sync::mpsc;
use tokio::time::Duration;

#[derive(Parser, Debug)]
#[command(version, about = "CPAL feedback example", long_about = None)]
struct Opt {
    /// The input audio device to use
    #[arg(short, long, value_name = "IN", default_value_t = String::from("default"))]
    input_device: String,

    /// The output audio device to use
    #[arg(short, long, value_name = "OUT", default_value_t = String::from("default"))]
    output_device: String,

    /// Specify the delay between input and output
    #[arg(short, long, value_name = "DELAY_MS", default_value_t = 150.0)]
    latency: f32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sample_rate = 1000;
    let threshold = 0.2;
    let opt = Opt::parse();

    #[cfg(any(
        not(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd"
        )),
        not(feature = "jack")
    ))]
    let host = cpal::default_host();

    // Find devices.
    let input_device = if opt.input_device == "default" {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|x| x.name().map(|y| y == opt.input_device).unwrap_or(false))
    }
    .expect("failed to find input device");

    println!("Using input device: \"{}\"", input_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let mut config: cpal::StreamConfig = input_device.default_input_config()?.into();
    config.sample_rate = cpal::SampleRate(sample_rate);

    let (tx_sample, mut rx_sample) = mpsc::channel(1);
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for &sample in data {
            let abs = sample.abs();
            if abs > threshold {
                if tx_sample.capacity() != 0 {
                    tx_sample.blocking_send(abs).unwrap();
                }
            }
        }
    };

    let sample_debounce_receiver = tokio::spawn(async move {
        loop {
            let sample = rx_sample.recv().await;
            println!("{}", sample.unwrap());
            tokio::time::sleep(Duration::from_secs(1)).await;
            // Flush one value after the sleep. TODO: Clean up this logic
            rx_sample.recv().await;
        }
    });

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        opt.latency
    );
    input_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(input_stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
