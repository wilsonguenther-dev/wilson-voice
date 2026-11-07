// yap-engine-ane — Parakeet on the Neural Engine, as a yap-asr look-alike
// (yap-1106-sp-10-ane-engine-spike).
//
// Speaks protocol 0.1.0 of desktop/src-tauri/src/asr_protocol.rs on
// stdin/stdout, so `engine_owner::HelperEngine` drives it unchanged: one ready
// line, then one response per request; `cancel` is answered by the in-flight
// decode's own (error) response; `shutdown` frees the model, answers, exits;
// stdin EOF frees the model and exits 0. stdout carries JSON only; one
// structured diagnostic line per event goes to stderr ("yap-engine-ane <event>
// key=value"), never transcript text, audio or a full path.
//
// The model directory is never downloaded here: `AsrModels.loadLocal` reads
// the Core ML bundles already on disk. `model_path` must be that directory;
// when it is not (the bench hands the app's GGUF path), YAP_ANE_MODEL_DIR
// names it instead.
import CoreML
import Darwin
import FluidAudio
import Foundation
import YapEngineANECore

setvbuf(stdout, nil, _IOLBF, 0)

let pid = UInt32(ProcessInfo.processInfo.processIdentifier)

func diag(_ line: String) {
    FileHandle.standardError.write(Data("yap-engine-ane \(line)\n".utf8))
}

final class Out: @unchecked Sendable {
    private let lock = NSLock()
    func line(_ s: String) {
        lock.lock()
        defer { lock.unlock() }
        FileHandle.standardOutput.write(Data((s + "\n").utf8))
    }
    func respond(_ r: AsrResponse) {
        do {
            line(try encodeLine(r))
        } catch {
            diag("encode_failed id=\(r.id)")
        }
    }
}

/// Peak resident memory of this process, in MB (macOS reports bytes).
func maxRssMb() -> Int {
    var usage = rusage()
    getrusage(RUSAGE_SELF, &usage)
    return Int(usage.ru_maxrss / (1024 * 1024))
}

let out = Out()

/// Shared with the stdin thread: the in-flight decode, so `cancel` can reach it.
final class Inflight: @unchecked Sendable {
    private let lock = NSLock()
    private var task: Task<ASRResult, Error>?
    private var cancelled = false

    func begin(_ t: Task<ASRResult, Error>) {
        lock.lock(); task = t; cancelled = false; lock.unlock()
    }
    func end() -> Bool {
        lock.lock(); defer { lock.unlock() }
        task = nil
        let was = cancelled
        cancelled = false
        return was
    }
    func cancel() {
        lock.lock(); defer { lock.unlock() }
        guard let task else { return }
        cancelled = true
        task.cancel()
    }
}

let inflight = Inflight()

final class Engine {
    let manager: AsrManager
    let decoderLayers: Int
    init(manager: AsrManager, decoderLayers: Int) {
        self.manager = manager
        self.decoderLayers = decoderLayers
    }
}

func resolveModelDir(_ requested: String?) -> URL? {
    var candidates: [String] = []
    if let requested { candidates.append(requested) }
    if let env = ProcessInfo.processInfo.environment["YAP_ANE_MODEL_DIR"], !env.isEmpty {
        candidates.append(env)
    }
    for path in candidates {
        var isDir: ObjCBool = false
        if FileManager.default.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue {
            let url = URL(fileURLWithPath: path, isDirectory: true)
            if AsrModels.modelsExist(at: url, version: .v3) {
                return url
            }
        }
    }
    return nil
}

let modelMissing =
    "The Neural Engine speech model is not on this Mac yet, so this engine cannot load."
let loadFailed = "The Neural Engine speech model could not be loaded. Switch back to the standard engine."
let notLoaded = "No speech model is loaded yet."
let decodeCancelled = "The transcription was stopped early."
let decodeFailed = "The Neural Engine could not transcribe this clip."

final class Worker: @unchecked Sendable {
    var engine: Engine?

    func load(_ req: AsrRequest) async -> AsrResponse {
        guard let dir = resolveModelDir(req.modelPath) else {
            diag("load_failed reason=model_missing")
            return .fail(req.id, modelMissing)
        }
        if let old = engine {
            await old.manager.cleanup()
            engine = nil
        }
        let t0 = Date()
        do {
            let models = try AsrModels.loadLocal(
                from: dir, version: .v3, configuration: AsrModels.defaultConfiguration())
            let manager = AsrManager(config: .default)
            try await manager.loadModels(models)
            let layers = await manager.decoderLayerCount
            engine = Engine(manager: manager, decoderLayers: layers)
            // First load on a Mac includes Core ML's ANE compile; later loads
            // hit the system cache. The bench reads this line for both.
            let ms = Int(Date().timeIntervalSince(t0) * 1000)
            diag("loaded load_ms=\(ms) compute=cpu_and_ne maxrss_mb=\(maxRssMb())")
            return AsrResponse(id: req.id, ok: true, caps: .parakeetAne())
        } catch {
            diag("load_failed reason=\(String(describing: type(of: error)))")
            return .fail(req.id, loadFailed)
        }
    }

    func transcribe(_ req: AsrRequest) async -> AsrResponse {
        guard let engine else { return .fail(req.id, notLoaded) }
        guard let path = req.samplesPath else { return .fail(req.id, "The request named no audio file.") }
        let samples: [Float]
        do {
            samples = try samplesFromLeBytes(try Data(contentsOf: URL(fileURLWithPath: path)))
        } catch {
            diag("decode_failed reason=samples_unreadable")
            return .fail(req.id, "The recorded audio could not be read.")
        }
        if samples.isEmpty {
            return AsrResponse(id: req.id, ok: true,
                               timed: buildTimed(text: "", tokens: nil, timed: req.timed), language: "en")
        }
        let manager = engine.manager
        let layers = engine.decoderLayers
        let t0 = Date()
        let task = Task<ASRResult, Error> {
            var state = TdtDecoderState.make(decoderLayers: layers)
            return try await manager.transcribe(samples, decoderState: &state)
        }
        inflight.begin(task)
        let result = await task.result
        let wasCancelled = inflight.end()
        let audioS = Double(samples.count) / 16_000.0
        let ms = Int(Date().timeIntervalSince(t0) * 1000)
        if wasCancelled {
            diag("decode_cancelled ms=\(ms) audio_s=\(String(format: "%.1f", audioS))")
            return .fail(req.id, decodeCancelled)
        }
        switch result {
        case .success(let r):
            let rtfx = ms > 0 ? audioS / (Double(ms) / 1000.0) : 0
            diag("decoded ms=\(ms) audio_s=\(String(format: "%.1f", audioS)) rtfx=\(String(format: "%.1f", rtfx)) "
                 + "tokens=\(r.tokenTimings?.count ?? 0) maxrss_mb=\(maxRssMb())")
            let tokens = r.tokenTimings?.map { TimedToken(text: $0.token, start: $0.startTime, end: $0.endTime) }
            return AsrResponse(id: req.id, ok: true,
                               timed: buildTimed(text: r.text, tokens: tokens, timed: req.timed), language: "en")
        case .failure(let error):
            diag("decode_failed ms=\(ms) reason=\(String(describing: type(of: error)))")
            return .fail(req.id, decodeFailed)
        }
    }

    func freeModel() async -> UInt32 {
        guard let engine else { return 0 }
        await engine.manager.cleanup()
        self.engine = nil
        return 1
    }
}

enum Work {
    case request(AsrRequest)
    case eof
}

let (stream, feed) = AsyncStream<Work>.makeStream()
let worker = Worker()

Task.detached {
    for await work in stream {
        switch work {
        case .eof:
            _ = await worker.freeModel()
            diag("exit reason=stdin_closed")
            exit(0)
        case .request(let req):
            switch req.kind {
            case Kind.load:
                out.respond(await worker.load(req))
            case Kind.transcribe:
                out.respond(await worker.transcribe(req))
            case Kind.shutdown:
                let freed = await worker.freeModel()
                out.respond(AsrResponse(id: req.id, ok: true, freed: freed))
                diag("exit reason=shutdown")
                exit(0)
            default:
                out.respond(.fail(req.id, "Unknown request kind."))
            }
        }
    }
}

if CommandLine.arguments.count > 1 {
    diag("bad_argument")
    FileHandle.standardError.write(Data("usage: yap-engine-ane   (the model is loaded by request, not by argv)\n".utf8))
    exit(2)
}

do {
    out.line(try encodeLine(AsrReady(pid: pid)))
} catch {
    exit(1)
}
diag("ready pid=\(pid)")

while let line = readLine(strippingNewline: true) {
    if line.trimmingCharacters(in: .whitespaces).isEmpty { continue }
    guard let req = try? JSONDecoder().decode(AsrRequest.self, from: Data(line.utf8)) else {
        diag("bad_request")
        out.respond(.fail(0, "The request could not be read."))
        continue
    }
    if req.kind == Kind.cancel {
        // Answered by the decode's own response, never by a line of its own.
        inflight.cancel()
        continue
    }
    feed.yield(.request(req))
}
feed.yield(.eof)
// The worker ends the process once the model is freed.
dispatchMain()
