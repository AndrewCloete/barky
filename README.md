# Barky
My wife and I need a mechanism to quantify how noisy the dogs are when we are
not home. This helps us validate the effectiveness of various toys, treats and
tricks to keep them happy on their own. This application detects the presence of
sound and reports it my Home Assistant MQTT broker for persistence in InfluxDB
and display.

Later, I plan to use this same mechanism to trigger the automatic dispense of a
treat if a sufficiently long period of silence is achieved (will link that
project with the 3D print files). My theory is that this will reinforce the good
behaviour.

This is also yet another project for my to improve my Rustiness.

## History
My first version of this project was to use [sox](https://sox.sourceforge.net/)
that can do threshold audio recording. Here is an extract of the bash script
that did the recording. 

```bash
# Record if sound is more than 2% for more than 0.1 seconds, and stop once less than 3% for 5 seconds. Split file
rec -c 1 -r 16k "$AUDIO_PATH/record.wav" silence 1 0.1 1% 1 5.0 1% : newfile : restart
```

This threshold-based audio recording is very useful. I've also seen it use to
efficiently record bird song.

Issues with this approach for bark detection:
- While it's nice to be able to play back the recorded sound, it was more
  difficult to quantify, visualise and monitor the result in real time while we
  were not home.
- The system had to be deactivated when we were at home to avoid filling
  storage. It is annoying to have to "arm" the system when we leave.
- Writing to disk continuously reduces the lifespan of the Raspberry Pi SD
  card.

In contrast, this application persists nothing to disk and effectively flattens
out audio to a simple boolean signal. The trade-off here is of coarse that there
is no recording to play back for further analysis.

# Basic operation
- Monitors the microphone and emits an MQTT message when sound is detected. 
- A periodic timer emits an MQTT message if no sound is detected.
- When sound is detected, the timer is reset.
- Both the sound threshold and the silence timeout is configurable.