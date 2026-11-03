# Device Layer & Multimodal Hardware Integration

AILang v3.0 introduces native abstractions for real-world devices, sensors, and actuators.

## 1. Device Declarations

```ail
device MainCamera camera {
    permissions ["read", "capture_stream"];
}

device EnvSensor sensor {
    permissions ["read", "calibrate"];
}

device AlertSpeaker speaker {
    permissions ["write", "stream_audio"];
}
```

Supported Device Kinds:
- `camera`
- `microphone`
- `speaker`
- `sensor`
- `gpu`

## 2. Kernel Device Abstraction

In the v3.0 foundation, device declarations establish permission boundaries and capability descriptors in the runtime environment.
Hardware drivers interface via the Kernel Device Runtime, isolating untrusted agent code from raw OS system calls.

