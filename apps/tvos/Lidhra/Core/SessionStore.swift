import Foundation
import Security

/// Keychain storage for the bridge's `session` JSON (provider, token and, for Real-Debrid
/// device logins, OAuth refresh material). One generic-password item.
enum SessionStore {
    static let service = "dev.peterdsp.lidhra.tv"
    private static let account = "session"

    private static var baseQuery: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }

    static func load() -> Data? {
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess else { return nil }
        return item as? Data
    }

    static func save(_ data: Data) throws {
        let update: [String: Any] = [kSecValueData as String: data]
        var status = SecItemUpdate(baseQuery as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var add = baseQuery
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw KeychainError(status: status) }
    }

    static func clear() {
        SecItemDelete(baseQuery as CFDictionary)
    }

    struct KeychainError: LocalizedError {
        let status: OSStatus
        var errorDescription: String? {
            Strings.errorKeychain(status)
        }
    }
}
