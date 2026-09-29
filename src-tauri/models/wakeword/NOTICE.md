# Wake-word models

These three files are openWakeWord's pre-trained "hey jarvis" model set, release v0.5.1:

- `melspectrogram.onnx`
- `embedding_model.onnx`
- `hey_jarvis_v0.1.onnx`

Source: https://github.com/dscripka/openWakeWord (David Scripka)

## License

openWakeWord's code is licensed under Apache 2.0. Its pre-trained models, including these
files, are licensed under the Creative Commons Attribution-NonCommercial-ShareAlike 4.0
International license (CC BY-NC-SA 4.0), because their training data includes datasets with
unknown or restrictive licensing.

That means these models may be used for personal, non-commercial purposes only. Before selling
or commercially distributing Jarvis, replace them with a model trained on data you have the
rights to (openWakeWord's training notebooks can produce one). The detector in
`src/wakeword.rs` loads whatever three files are here, so swapping them needs no code changes as
long as the input and output shapes stay the same.

License text: https://creativecommons.org/licenses/by-nc-sa/4.0/
