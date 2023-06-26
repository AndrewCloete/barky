use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use rumqttc::{AsyncClient, MqttOptions, QoS};

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::Duration;

#[derive(Parser, Debug)]
#[command(version, about = "CPAL feedback example", long_about = None)]
struct Opt {
    /// The input audio device to use
    #[arg(short, long, value_name = "IN", default_value_t = String::from("default"))]
    input_device: String,

    #[arg(short, long, value_name = "THRESHOLD", default_value_t = 0.2)]
    threshold: f32,

    #[arg(
        short,
        long,
        value_name = "MQTT_HOST",
        default_value_t = String::from("homeassistant.local")
    )]
    broker_mqtt: String,

    #[arg(short, long, value_name = "MQTT_USERNAME")]
    username_mqtt: String,

    #[arg(short, long, value_name = "MQTT_PASSWORD")]
    password_mqtt: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let sample_rate = 1000; // We can get away with a very low sample rate since we are not interested in the audio, only in "loud events".

    println!("{:?}", &opt);
    let mut mqttoptions = MqttOptions::new("rumqtt-sync", opt.broker_mqtt, 1883);
    mqttoptions.set_credentials(opt.username_mqtt, opt.password_mqtt);
    mqttoptions.set_keep_alive(Duration::from_secs(20));
    mqttoptions.set_transport(rumqttc::Transport::Tcp);
    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

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

    host.input_devices()
        .unwrap()
        .for_each(|d| println!("{:?}", d.name()));

    // Find devices.
    let input_device = if opt.input_device == "default" {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|x| x.name().map(|y| y == opt.input_device).unwrap_or(false))
    }
    .expect("failed to find input device");

    println!("Using input device: \"{}\"", input_device.name()?);

    let mut config: cpal::StreamConfig = input_device.default_input_config()?.into();
    config.sample_rate = cpal::SampleRate(sample_rate);

    let (tx_sample, mut rx_sample) = mpsc::channel(1);
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for &sample in data {
            let abs = sample.abs();
            if abs > opt.threshold {
                if tx_sample.capacity() != 0 {
                    tx_sample.blocking_send(abs).unwrap();
                }
            }
        }
    };

    // Build streams.
    println!("Attempting to build streams `{:?}`.", config);
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!("Starting the input stream",);
    input_stream.play()?;
    let mq_client_1 = client.clone();

    let (tx_bark, mut rx_bark) = mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            let sample = rx_sample.recv().await;
            println!("{}", sample.unwrap());
            match mq_client_1
                .publish("casa/bark", QoS::AtLeastOnce, false, "1")
                .await
            {
                Ok(_) => {
                    println!("Sent: Bark!");
                    tx_bark.send("bark!").await.unwrap();
                }
                Err(e) => println!("{}", e),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            // Flush one value after the sleep. TODO: Clean up this logic
            rx_sample.recv().await;
        }
    });

    // Timer for no-bark
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60 * 5)).await;
            match rx_bark.try_recv() {
                Ok(_) => println!("No bark timer reset"),
                Err(TryRecvError::Empty) => {
                    match client
                        .publish("casa/bark", QoS::AtLeastOnce, false, "0")
                        .await
                    {
                        Ok(_) => {
                            println!("Sent: No bark!");
                        }
                        Err(e) => println!("{}", e),
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    println!("Whut?");
                }
            }
        }
    });
    println!("Loop forever");
    loop {
        let notification = eventloop.poll().await.unwrap();
        println!("Received = {:?}", notification);
    }
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
