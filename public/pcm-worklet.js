// Captures microphone audio and converts it to 16 kHz, 16-bit PCM for Gemini Live.
// Posts one Int16Array buffer every ~40 ms.
class PcmRecorder extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const target = (options.processorOptions && options.processorOptions.targetRate) || 16000;
    this.ratio = sampleRate / target;
    this.size = Math.round(target * 0.04);
    this.out = new Int16Array(this.size);
    this.n = 0;
    this.phase = 0;
    this.sum = 0;
    this.count = 0;
  }
  process(inputs) {
    const input = inputs[0] && inputs[0][0];
    if (!input) return true;
    for (let i = 0; i < input.length; i++) {
      this.sum += input[i];
      this.count++;
      this.phase += 1;
      if (this.phase >= this.ratio) {
        this.phase -= this.ratio;
        const v = Math.max(-1, Math.min(1, this.sum / this.count));
        this.out[this.n++] = v < 0 ? v * 0x8000 : v * 0x7fff;
        this.sum = 0;
        this.count = 0;
        if (this.n === this.size) {
          this.port.postMessage(this.out.buffer, [this.out.buffer]);
          this.out = new Int16Array(this.size);
          this.n = 0;
        }
      }
    }
    return true;
  }
}
registerProcessor("pcm-recorder", PcmRecorder);
