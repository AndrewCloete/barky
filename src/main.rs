use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::env::var;
use std::fs;

use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Outgoing, QoS};

use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::Duration;

#[derive(Parser, Debug)]
#[command(version, about = "CPAL feedback example", long_about = None)]
struct Opt {
    /// The input audio device to use
    #[arg(short, long, value_name = "IN", default_value_t = String::from("default"))]
    input_device: String,
}

#[derive(Debug, Deserialize, Clone)]
struct MqttConfig {
    broker: String,
    username: String,
    password: String,
    port: u16,
    keepalive_sec: u64,
    bark_topic: String,
    no_bark_topic: String,
}

#[derive(Debug, Deserialize, Clone)]
struct ConfigFile {
    mqtt: MqttConfig,
    threshold: f32,
    sample_rate: u32,
    no_bark_seconds: u64,
}

fn read_config() -> Result<ConfigFile, Box<dyn std::error::Error>> {
    let home_path = var("HOME").expect("$HOME not defined");
    let content = fs::read_to_string(format!("{}/.barky.json", home_path))?;
    let config: ConfigFile = serde_json::from_str(&content)?;
    Ok(config)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    println!("{:?}", opt);

    let file_config = read_config().unwrap();
    println!("{:?}", file_config);

    let sample_rate = file_config.sample_rate; // We can get away with a very low sample rate since we are not interested in the audio, only in "loud events".
    let bark_topic = file_config.mqtt.bark_topic;
    let no_bark_topic = file_config.mqtt.no_bark_topic;
    let no_bark_seconds = file_config.no_bark_seconds;
    let threshold = file_config.threshold;

    let mut mqttoptions = MqttOptions::new(
        "rumqtt-sync",
        file_config.mqtt.broker,
        file_config.mqtt.port,
    );
    mqttoptions.set_credentials(file_config.mqtt.username, file_config.mqtt.password);
    mqttoptions.set_keep_alive(Duration::from_secs(file_config.mqtt.keepalive_sec));
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
            if abs > threshold {
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
            let sample = (rx_sample.recv().await).expect("Could not unwrap sample");
            println!("{}", sample);
            match mq_client_1
                .publish(&bark_topic, QoS::AtLeastOnce, false, sample.to_string())
                .await
            {
                Ok(_) => {
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
            tokio::time::sleep(Duration::from_secs(no_bark_seconds)).await;
            match rx_bark.try_recv() {
                Ok(_) => println!("No bark timer reset"),
                Err(TryRecvError::Empty) => {
                    match client
                        .publish(&no_bark_topic, QoS::AtLeastOnce, false, "0")
                        .await
                    {
                        Ok(_) => (),
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
        match notification {
            // Don't print the pings
            Event::Outgoing(Outgoing::PingReq) => (),
            Event::Incoming(Incoming::PingResp) => (),
            _ => println!("rx {:?}", notification),
        }
    }
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
