import Foundation
import LidhraBridge

/// Errors surfaced by the Rust core or by talking to it.
enum LidhraError: LocalizedError, Equatable {
    /// The bridge answered `{"error": "..."}` (bad input, provider or network failure).
    case bridge(String)
    /// The bridge did not answer within `LidhraCore.timeout`.
    case timedOut(command: String)
    /// Arguments could not be encoded, or the result could not be decoded.
    case badData(command: String)

    var errorDescription: String? {
        switch self {
        case .bridge(let message): return message
        case .timedOut: return Strings.errorTimedOut
        case .badData(let command): return Strings.errorBadData(command)
        }
    }

    /// Decode the `{"error": "..."}` payload the C ABI sends with `ok == false`.
    static func fromErrorPayload(_ data: Data) -> LidhraError {
        let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        if let message = (object as? [String: Any])?["error"] as? String, !message.isEmpty {
            return .bridge(message)
        }
        return .bridge(String(decoding: data, as: UTF8.self))
    }
}

/// Swift face of `lidhra-bridge` (see crates/lidhra-bridge/src/lib.rs for the command table).
///
/// Every command is one `lidhra_bridge_call`: the arguments go in as a JSON object, the result
/// comes back later on a Rust worker thread through a C callback. Each call owns a
/// `PendingCall` box that is passed to Rust as the opaque `ctx` pointer (retained once) and
/// released by the callback, which resumes the Swift continuation. Because `call` is
/// main-actor isolated, callers always continue on the main actor.
@MainActor
final class LidhraCore {
    static let shared = LidhraCore()

    /// Upper bound for one command. `links` unrestricts every file of a transfer in turn, so
    /// this is generous; it only guards against a callback that never arrives.
    static let timeout: Duration = .seconds(120)

    private let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }()

    /// Version of the linked Rust core, e.g. "0.1.0".
    nonisolated static var version: String { String(cString: lidhra_bridge_version()) }

    /// Run a command and return its JSON result as Foundation objects
    /// (`[String: Any]`, `[Any]`, `String`, `NSNumber` or `NSNull`).
    func call(_ name: String, _ args: [String: Any] = [:]) async throws -> Any {
        let data = try await callRaw(name, args)
        do {
            return try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        } catch {
            throw LidhraError.badData(command: name)
        }
    }

    /// Run a command and decode its result (snake_case keys map to camelCase properties).
    func call<T: Decodable>(_ name: String, _ args: [String: Any] = [:], as type: T.Type) async throws -> T {
        let data = try await callRaw(name, args)
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw LidhraError.badData(command: name)
        }
    }

    /// Run a command and return the raw UTF-8 JSON result.
    func callRaw(_ name: String, _ args: [String: Any] = [:]) async throws -> Data {
        let argsJSON: String?
        if args.isEmpty {
            argsJSON = nil
        } else {
            guard JSONSerialization.isValidJSONObject(args),
                  let data = try? JSONSerialization.data(withJSONObject: args) else {
                throw LidhraError.badData(command: name)
            }
            argsJSON = String(decoding: data, as: UTF8.self)
        }

        return try await withCheckedThrowingContinuation { continuation in
            let pending = PendingCall(continuation)
            pending.armTimeout(Self.timeout, command: name)
            // +1 retain owned by the Rust side; balanced by `takeRetainedValue` in the callback.
            let ctx = Unmanaged.passRetained(pending).toOpaque()
            // Rust copies both strings before `lidhra_bridge_call` returns.
            name.withCString { cName in
                if let argsJSON {
                    argsJSON.withCString { cArgs in lidhra_bridge_call(cName, cArgs, ctx, lidhraCallback) }
                } else {
                    lidhra_bridge_call(cName, nil, ctx, lidhraCallback)
                }
            }
        }
    }
}

/// One in-flight bridge call. The continuation is resumed exactly once, by whichever of the
/// Rust callback or the timeout gets here first; the other finds it already taken.
private final class PendingCall: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Data, Error>?
    private var timer: Task<Void, Never>?

    init(_ continuation: CheckedContinuation<Data, Error>) {
        self.continuation = continuation
    }

    func armTimeout(_ duration: Duration, command: String) {
        // Weak: once the callback has run and released the box there is nothing to time out.
        let timer = Task.detached { [weak self] in
            try? await Task.sleep(for: duration)
            guard !Task.isCancelled else { return }
            self?.finish(.failure(LidhraError.timedOut(command: command)))
        }
        lock.withLock { self.timer = timer }
    }

    func finish(_ result: Result<Data, Error>) {
        let (continuation, timer) = lock.withLock {
            defer { self.continuation = nil; self.timer = nil }
            return (self.continuation, self.timer)
        }
        timer?.cancel()
        continuation?.resume(with: result)
    }
}

/// The C callback handed to Rust. Runs on a Rust worker thread; `json` is only valid for the
/// duration of the call, so it is copied before anything else happens.
private let lidhraCallback: LidhraCallback = { ctx, ok, json in
    guard let ctx else { return }
    let pending = Unmanaged<PendingCall>.fromOpaque(ctx).takeRetainedValue()
    let data = json.map { Data(bytes: $0, count: strlen($0)) } ?? Data()
    pending.finish(ok ? .success(data) : .failure(LidhraError.fromErrorPayload(data)))
}
