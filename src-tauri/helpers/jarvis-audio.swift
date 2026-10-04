// jarvis-audio: Jarvis's native audio helper, one small program with three jobs.
//
//   jarvis-audio syscap <jarvis pid>   Records what the Mac is playing (the other side of a call),
//                                       leaving out Jarvis's own sounds, and writes 16 kHz mono
//                                       16-bit PCM to stdout. Needs macOS 14.4 or later.
//   jarvis-audio vpio [mic name]       The microphone with Apple's echo cancellation, written to
//                                       stdout as 16 kHz mono 16-bit PCM, while Jarvis's voice
//                                       (24 kHz mono 16-bit PCM from stdin) plays through the same
//                                       audio engine, so the canceller knows exactly what to remove.
//   jarvis-audio route                  The current input and output devices, as one line of JSON.
//
// Status lines go to stderr, one per line:
//   ready <details>            running
//   error <sentence>           about to exit with a failure; the sentence is shown to the user
//   warn <sentence>            worth logging, not fatal
//   level <rms> <frames done>  vpio, every 50 ms while Jarvis's voice is playing
//   idle <frames done>         vpio, when the playback queue has drained
//
// vpio's stdin is framed: a 4-byte little-endian length, then that many bytes of PCM. A length of 0
// drops everything queued (barge-in). Doing this in the same stream as the audio, rather than with a
// signal, means audio already on its way can't sneak in after the flush. SIGUSR1 also flushes.
//
// The helper quits when stdin closes (vpio), when nobody reads stdout any more, or when Jarvis
// (its parent process) exits, so it never outlives the app.
//
// Built by build.rs with `swiftc -O`; the deployment target is macOS 13, so the system-audio parts
// are behind availability checks.

import AVFoundation
import CoreAudio
import Darwin
import Foundation

var keepAlive: [AnyObject] = []
/// Undo whatever the current mode set up (aggregate devices, taps) before exiting.
var cleanup: () -> Void = {}

func status(_ line: String) {
    FileHandle.standardError.write((line + "\n").data(using: .utf8)!)
}

func fail(_ sentence: String) -> Never {
    cleanup()
    status("error \(sentence)")
    exit(1)
}

func quit() -> Never {
    cleanup()
    exit(0)
}

/// Write all `n` bytes, or report that the reader has gone away.
func writeAll(_ fd: Int32, _ p: UnsafeRawPointer, _ n: Int) -> Bool {
    var off = 0
    while off < n {
        let w = write(fd, p + off, n - off)
        if w < 0 {
            if errno == EINTR { continue }
            return false
        }
        off += w
    }
    return true
}

/// Read exactly `n` bytes, or nothing if the stream ended first.
func readExact(_ fd: Int32, _ buf: UnsafeMutableRawPointer, _ n: Int) -> Bool {
    var off = 0
    while off < n {
        let r = read(fd, buf + off, n - off)
        if r < 0 && errno == EINTR { continue }
        if r <= 0 { return false }
        off += r
    }
    return true
}

func onQuitSignals() {
    for sig in [SIGINT, SIGTERM] {
        signal(sig, SIG_IGN)
        let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
        src.setEventHandler { quit() }
        src.resume()
        keepAlive.append(src)
    }
}

/// Exit when Jarvis does, so a crash or force-quit never leaves the helper recording.
func quitWithParent() {
    let parent = getppid()
    guard parent > 1 else { return }
    let src = DispatchSource.makeProcessSource(identifier: parent, eventMask: .exit, queue: .main)
    src.setEventHandler { quit() }
    src.resume()
    keepAlive.append(src)
}

// MARK: Core Audio properties

let systemObject = AudioObjectID(kAudioObjectSystemObject)

func address(_ sel: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: sel, mScope: scope, mElement: kAudioObjectPropertyElementMain)
}

func getProp<T>(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector, _ value: inout T,
                scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> OSStatus {
    var addr = address(sel, scope)
    var size = UInt32(MemoryLayout<T>.size)
    return withUnsafeMutablePointer(to: &value) { AudioObjectGetPropertyData(obj, &addr, 0, nil, &size, $0) }
}

func stringProp(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector) -> String? {
    var ref: Unmanaged<CFString>?
    guard getProp(obj, sel, &ref) == noErr, let s = ref?.takeRetainedValue() else { return nil }
    return s as String
}

func objectList(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector) -> [AudioObjectID] {
    var addr = address(sel)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(obj, &addr, 0, nil, &size) == noErr, size > 0 else { return [] }
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(obj, &addr, 0, nil, &size, &ids) == noErr else { return [] }
    return ids
}

func defaultDevice(_ sel: AudioObjectPropertySelector) -> AudioDeviceID {
    var id = AudioDeviceID(kAudioObjectUnknown)
    _ = getProp(systemObject, sel, &id)
    return id
}

func hasInput(_ dev: AudioDeviceID) -> Bool {
    var addr = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput)
    var size: UInt32 = 0
    return AudioObjectGetPropertyDataSize(dev, &addr, 0, nil, &size) == noErr && size > 0
}

func transport(_ dev: AudioDeviceID) -> String {
    var t: UInt32 = 0
    guard getProp(dev, kAudioDevicePropertyTransportType, &t) == noErr else { return "" }
    switch t {
    case kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE: return "bluetooth"
    case kAudioDeviceTransportTypeBuiltIn: return "builtin"
    case kAudioDeviceTransportTypeUSB: return "usb"
    case kAudioDeviceTransportTypeHDMI, kAudioDeviceTransportTypeDisplayPort: return "display"
    case kAudioDeviceTransportTypeAggregate: return "aggregate"
    case kAudioDeviceTransportTypeVirtual: return "virtual"
    case kAudioDeviceTransportTypeAirPlay: return "airplay"
    default: return "other"
    }
}

func inputDevice(named name: String) -> AudioDeviceID? {
    objectList(systemObject, kAudioHardwarePropertyDevices).first { dev in
        hasInput(dev) && stringProp(dev, kAudioObjectPropertyName) == name
    }
}

// MARK: route

func runRoute() -> Never {
    func describe(_ sel: AudioObjectPropertySelector) -> [String: String] {
        let dev = defaultDevice(sel)
        guard dev != kAudioObjectUnknown else { return ["name": "", "transport": ""] }
        return ["name": stringProp(dev, kAudioObjectPropertyName) ?? "", "transport": transport(dev)]
    }
    let route = ["input": describe(kAudioHardwarePropertyDefaultInputDevice),
                 "output": describe(kAudioHardwarePropertyDefaultOutputDevice)]
    let data = (try? JSONSerialization.data(withJSONObject: route)) ?? Data("{}".utf8)
    FileHandle.standardOutput.write(data + Data("\n".utf8))
    exit(0)
}

// MARK: syscap

typealias ResponsibleFn = @convention(c) (pid_t) -> pid_t

/// The app a process belongs to as far as macOS privacy is concerned. WebKit plays a web view's
/// sound in its own helper processes, which aren't Jarvis's children but are its responsibility.
/// (A private libSystem function; without it only Jarvis and its children are left out.)
let responsibleFor: ResponsibleFn? = {
    guard let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid") else { return nil }
    return unsafeBitCast(sym, to: ResponsibleFn.self)
}()

func parentOf(_ pid: pid_t) -> pid_t {
    var info = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.stride
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
    guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return 0 }
    return info.kp_eproc.e_ppid
}

/// Core Audio's objects for Jarvis and every process playing sound on its behalf.
@available(macOS 14.4, *)
func jarvisAudioObjects(_ jarvis: pid_t) -> [AudioObjectID] {
    guard jarvis > 0 else { return [] }
    let jarvisOwner = responsibleFor?(jarvis) ?? 0
    return objectList(systemObject, kAudioHardwarePropertyProcessObjectList).filter { obj in
        var pid: pid_t = -1
        guard getProp(obj, kAudioProcessPropertyPID, &pid) == noErr, pid > 0 else { return false }
        if pid == jarvis || pid == getpid() || parentOf(pid) == jarvis { return true }
        if let owner = responsibleFor?(pid) {
            // When run from a terminal during development, Jarvis's "owner" is the terminal;
            // its web view helpers then share that owner.
            if owner == jarvis || (jarvisOwner > 1 && owner == jarvisOwner) { return true }
        }
        return false
    }
}

@available(macOS 14.4, *)
func runSyscap(jarvis: pid_t) {
    // 1. A private tap on everything the Mac plays, except Jarvis, without muting the speakers.
    var excluded = jarvisAudioObjects(jarvis)
    let tapDesc = CATapDescription(stereoGlobalTapButExcludeProcesses: excluded)
    tapDesc.uuid = UUID()
    tapDesc.name = "Jarvis meeting recorder"
    tapDesc.isPrivate = true
    tapDesc.muteBehavior = .unmuted
    var tapID = AudioObjectID(kAudioObjectUnknown)
    var err = AudioHardwareCreateProcessTap(tapDesc, &tapID)
    guard err == noErr else {
        fail("macOS wouldn't let Jarvis listen to the Mac's sound (error \(err)). Allow Jarvis under System Settings → Privacy & Security → Screen & System Audio Recording.")
    }
    var aggID = AudioObjectID(kAudioObjectUnknown)
    var procID: AudioDeviceIOProcID?
    cleanup = {
        if let procID {
            AudioDeviceStop(aggID, procID)
            AudioDeviceDestroyIOProcID(aggID, procID)
        }
        if aggID != kAudioObjectUnknown { AudioHardwareDestroyAggregateDevice(aggID) }
        AudioHardwareDestroyProcessTap(tapID)
    }

    // 2. The tap's format (usually 48 kHz stereo float).
    var asbd = AudioStreamBasicDescription()
    err = getProp(tapID, kAudioTapPropertyFormat, &asbd)
    guard err == noErr, let tapFormat = AVAudioFormat(streamDescription: &asbd) else {
        fail("Couldn't read the format of the Mac's sound (error \(err)).")
    }

    // 3. A private aggregate device holding the tap, clocked by the current speakers.
    let outDev = defaultDevice(kAudioHardwarePropertyDefaultSystemOutputDevice)
    guard outDev != kAudioObjectUnknown, let outUID = stringProp(outDev, kAudioDevicePropertyDeviceUID) else {
        fail("There's no sound output device to record from. Pick one under System Settings → Sound → Output.")
    }
    let aggDesc: [String: Any] = [
        kAudioAggregateDeviceNameKey: "Jarvis meeting recorder",
        kAudioAggregateDeviceUIDKey: UUID().uuidString,
        kAudioAggregateDeviceMainSubDeviceKey: outUID,
        kAudioAggregateDeviceIsPrivateKey: true,
        kAudioAggregateDeviceIsStackedKey: false,
        kAudioAggregateDeviceTapAutoStartKey: true,
        kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outUID]],
        kAudioAggregateDeviceTapListKey: [[kAudioSubTapDriftCompensationKey: true,
                                           kAudioSubTapUIDKey: tapDesc.uuid.uuidString]],
    ]
    err = AudioHardwareCreateAggregateDevice(aggDesc as CFDictionary, &aggID)
    guard err == noErr else { fail("Couldn't set up recording of the Mac's sound (error \(err)).") }

    // 4. Convert to 16 kHz mono 16-bit and write it out.
    let outFormat = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true)!
    guard let converter = AVAudioConverter(from: tapFormat, to: outFormat) else {
        fail("Couldn't convert the Mac's sound (\(tapFormat)).")
    }
    converter.downmix = true
    let ioQueue = DispatchQueue(label: "jarvis-audio.syscap", qos: .userInitiated)
    err = AudioDeviceCreateIOProcIDWithBlock(&procID, aggID, ioQueue) { _, inInputData, _, _, _ in
        guard let inBuf = AVAudioPCMBuffer(pcmFormat: tapFormat, bufferListNoCopy: inInputData, deallocator: nil),
              inBuf.frameLength > 0 else { return }
        let cap = AVAudioFrameCount(Double(inBuf.frameLength) * 16_000 / tapFormat.sampleRate) + 32
        guard let outBuf = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: cap) else { return }
        var fed = false
        var cerr: NSError?
        _ = converter.convert(to: outBuf, error: &cerr) { _, st in
            if fed { st.pointee = .noDataNow; return nil }
            fed = true
            st.pointee = .haveData
            return inBuf
        }
        let n = Int(outBuf.frameLength) * 2
        if n > 0, let p = outBuf.int16ChannelData?[0], !writeAll(STDOUT_FILENO, p, n) {
            DispatchQueue.main.async { quit() } // Jarvis stopped reading
        }
    }
    guard err == noErr, procID != nil else { fail("Couldn't start recording the Mac's sound (error \(err)).") }

    // 5. Keep leaving Jarvis out as its web view starts or stops making sound.
    var listAddr = address(kAudioHardwarePropertyProcessObjectList)
    let listener: AudioObjectPropertyListenerBlock = { _, _ in
        let now = jarvisAudioObjects(jarvis)
        guard Set(now) != Set(excluded) else { return }
        excluded = now
        tapDesc.processes = now
        var desc = tapDesc
        var a = address(kAudioTapPropertyDescription)
        let st = withUnsafePointer(to: &desc) {
            AudioObjectSetPropertyData(tapID, &a, 0, nil, UInt32(MemoryLayout<CATapDescription>.size), $0)
        }
        if st != noErr { status("warn couldn't update which of Jarvis's sounds to leave out (\(st))") }
    }
    AudioObjectAddPropertyListenerBlock(systemObject, &listAddr, DispatchQueue.main, listener)

    status("ready \(Int(tapFormat.sampleRate)) Hz, \(tapFormat.channelCount) ch, leaving out \(excluded.count) Jarvis process(es)")
    err = AudioDeviceStart(aggID, procID)
    guard err == noErr else { fail("Couldn't start recording the Mac's sound (error \(err)).") }
}

// MARK: vpio

let playFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 24_000, channels: 1, interleaved: false)!
let micOutFormat = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true)!
let noMicrophone = "No microphone is connected. Plug in a headset or USB mic, connect AirPods, or pick an input under System Settings → Sound → Input."

final class Playback {
    /// Frames received on stdin, and frames played (or dropped by a flush), since the start.
    var received: Int64 = 0
    var done: Int64 = 0
    /// Bumped on every flush, so buffers dropped by it don't count as played later.
    var generation = 0
    var level: Float = 0
    var speaking = false
}

enum VoiceError: Error {
    case noMicrophone
}

func rms(_ p: UnsafePointer<Float>, _ n: Int) -> Float {
    guard n > 0 else { return 0 }
    var sum: Float = 0
    for i in 0..<n { sum += p[i] * p[i] }
    return (sum / Float(n)).squareRoot()
}

/// One running audio engine with voice processing: the microphone, and a player for Jarvis's voice
/// going out through the same engine.
final class VoiceEngine {
    let engine = AVAudioEngine()
    let player = AVAudioPlayerNode()
    private(set) var micRate: Double = 0
    private var observer: NSObjectProtocol?

    init(micName: String, onLevel: @escaping (Float) -> Void, onChange: @escaping () -> Void) throws {
        let input = engine.inputNode
        do {
            // A specific microphone, if one was picked in Settings and it isn't the default anyway.
            let defaultInput = defaultDevice(kAudioHardwarePropertyDefaultInputDevice)
            if !micName.isEmpty, let dev = inputDevice(named: micName), dev != defaultInput, let unit = input.audioUnit {
                var id = dev
                let st = AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                              &id, UInt32(MemoryLayout<AudioDeviceID>.size))
                if st != noErr { status("warn couldn't switch to the microphone \"\(micName)\" (\(st)); using the default input") }
            }

            // 1. Voice processing has to be switched on while the engine is stopped; it covers output too.
            try input.setVoiceProcessingEnabled(true)
            if #available(macOS 14.0, *) {
                // By default voice processing turns all other sound on the Mac way down; keep that minimal.
                input.voiceProcessingOtherAudioDuckingConfiguration =
                    AVAudioVoiceProcessingOtherAudioDuckingConfiguration(enableAdvancedDucking: false, duckingLevel: .min)
            }

            // 2. Playback: 24 kHz mono float into the main mixer, which resamples for the speakers.
            engine.attach(player)
            engine.connect(player, to: engine.mainMixerNode, format: playFormat)

            // 3. Capture. The format is read after enabling voice processing (it may report several
            // channels; channel 0 is the processed voice).
            let hw = input.outputFormat(forBus: 0)
            guard hw.sampleRate > 0, hw.channelCount > 0 else { throw VoiceError.noMicrophone }
            micRate = hw.sampleRate
            let monoIn = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: hw.sampleRate, channels: 1, interleaved: false)!
            guard let converter = AVAudioConverter(from: monoIn, to: micOutFormat) else { throw VoiceError.noMicrophone }
            input.installTap(onBus: 0, bufferSize: 1024, format: hw) { buf, _ in
                guard let src = buf.floatChannelData, buf.frameLength > 0,
                      let mono = AVAudioPCMBuffer(pcmFormat: monoIn, frameCapacity: buf.frameLength) else { return }
                mono.frameLength = buf.frameLength
                mono.floatChannelData![0].update(from: src[0], count: Int(buf.frameLength))
                let cap = AVAudioFrameCount(Double(buf.frameLength) * 16_000 / hw.sampleRate) + 32
                guard let out = AVAudioPCMBuffer(pcmFormat: micOutFormat, frameCapacity: cap) else { return }
                var fed = false
                var cerr: NSError?
                _ = converter.convert(to: out, error: &cerr) { _, st in
                    if fed { st.pointee = .noDataNow; return nil }
                    fed = true
                    st.pointee = .haveData
                    return mono
                }
                let n = Int(out.frameLength) * 2
                if n > 0, let p = out.int16ChannelData?[0], !writeAll(STDOUT_FILENO, p, n) {
                    DispatchQueue.main.async { quit() } // Jarvis stopped reading
                }
            }

            // How loud Jarvis's voice is right now, for the orb.
            player.installTap(onBus: 0, bufferSize: 1200, format: nil) { buf, _ in
                guard let p = buf.floatChannelData?[0] else { return }
                onLevel(rms(p, Int(buf.frameLength)))
            }

            engine.prepare()
            try engine.start()
            player.play()
        } catch {
            stop()
            throw error
        }
        // Plugging in headphones or switching the mic reconfigures the engine and stops it.
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) { _ in onChange() }
    }

    func stop() {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        observer = nil
        engine.inputNode.removeTap(onBus: 0)
        player.removeTap(onBus: 0)
        engine.stop()
    }
}

func runVpio(micName: String) {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .denied, .restricted:
        fail("Jarvis isn't allowed to use the microphone. Turn on Jarvis under System Settings → Privacy & Security → Microphone.")
    default: break
    }
    let defaultInput = defaultDevice(kAudioHardwarePropertyDefaultInputDevice)
    guard defaultInput != kAudioObjectUnknown, hasInput(defaultInput) else { fail(noMicrophone) }

    let state = Playback()
    let q = DispatchQueue(label: "jarvis-audio.playback")
    /// The engine running now; only touched on `q`.
    var current: VoiceEngine?
    var restartPending = false

    func onLevel(_ r: Float) { q.async { state.level = r } }

    // Bluetooth headsets switch to their call profile when the microphone opens, which can make the
    // first start fail or reconfigure the engine right after it starts, so both are retried.
    func startEngine() -> VoiceEngine {
        var last = ""
        for attempt in 0..<4 {
            if attempt > 0 { Thread.sleep(forTimeInterval: 0.7) }
            do {
                return try VoiceEngine(micName: micName, onLevel: onLevel, onChange: scheduleRestart)
            } catch VoiceError.noMicrophone {
                fail(noMicrophone)
            } catch {
                last = error.localizedDescription
                status("warn starting the microphone with echo cancellation failed (try \(attempt + 1)): \(last)")
            }
        }
        fail("The microphone couldn't start with echo cancellation (\(last)). Try again, or turn Echo cancellation off in Settings.")
    }

    func scheduleRestart() {
        guard !restartPending else { return }
        restartPending = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            restartPending = false
            q.sync {
                current?.stop()
                current = nil
                state.generation += 1
                state.done = state.received
                state.level = 0
            }
            let fresh = startEngine()
            q.sync { current = fresh }
            status("warn the audio devices changed, so echo cancellation restarted")
        }
    }

    func flush() {
        state.generation += 1
        current?.player.stop()
        current?.player.play()
        state.done = state.received
        state.level = 0
    }

    let first = startEngine()
    q.sync { current = first }
    cleanup = { q.sync { current?.stop() } }

    signal(SIGUSR1, SIG_IGN)
    let usr1 = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: q)
    usr1.setEventHandler { flush() }
    usr1.resume()
    keepAlive.append(usr1)

    // Report the level while speaking, and when the queue has drained.
    let timer = DispatchSource.makeTimerSource(queue: q)
    timer.schedule(deadline: .now(), repeating: .milliseconds(50))
    timer.setEventHandler {
        if state.done < state.received {
            state.speaking = true
            status(String(format: "level %.4f %lld", state.level, state.done))
        } else if state.speaking {
            state.speaking = false
            state.level = 0
            status("idle \(state.done)")
        }
    }
    timer.resume()
    keepAlive.append(timer)

    // 4. stdin: framed 24 kHz 16-bit PCM to play, or a zero length to flush.
    Thread.detachNewThread {
        var header = [UInt8](repeating: 0, count: 4)
        while readExact(STDIN_FILENO, &header, 4) {
            let n = Int(UInt32(header[0]) | UInt32(header[1]) << 8 | UInt32(header[2]) << 16 | UInt32(header[3]) << 24)
            if n == 0 {
                q.sync { flush() }
                continue
            }
            var bytes = [UInt8](repeating: 0, count: n)
            guard readExact(STDIN_FILENO, &bytes, n) else { break }
            let frames = n / 2
            guard frames > 0, let pb = AVAudioPCMBuffer(pcmFormat: playFormat, frameCapacity: AVAudioFrameCount(frames)) else { continue }
            pb.frameLength = AVAudioFrameCount(frames)
            let dst = pb.floatChannelData![0]
            bytes.withUnsafeBytes { raw in
                for i in 0..<frames {
                    let s = Int16(bitPattern: UInt16(raw[2 * i]) | UInt16(raw[2 * i + 1]) << 8)
                    dst[i] = Float(s) / 32768
                }
            }
            q.sync {
                state.received += Int64(frames)
                guard let player = current?.player else {
                    state.done += Int64(frames) // restarting: this bit of speech is dropped
                    return
                }
                let gen = state.generation
                player.scheduleBuffer(pb, completionCallbackType: .dataPlayedBack) { _ in
                    q.async { if state.generation == gen { state.done += Int64(frames) } }
                }
            }
        }
        // stdin closed: Jarvis is done with the microphone.
        DispatchQueue.main.async { quit() }
    }

    status("ready \(Int(first.micRate)) Hz mic, voice processing on")
}

// MARK: main

signal(SIGPIPE, SIG_IGN)
onQuitSignals()
quitWithParent()

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "" {
case "route":
    runRoute()
case "syscap":
    if #available(macOS 14.4, *) {
        runSyscap(jarvis: args.count > 2 ? pid_t(args[2]) ?? 0 : 0)
    } else {
        fail("Recording the other side of calls needs macOS 14.4 or later.")
    }
case "vpio":
    runVpio(micName: args.count > 2 ? args[2] : "")
default:
    fail("usage: jarvis-audio syscap <jarvis pid> | vpio [mic name] | route")
}
dispatchMain()
