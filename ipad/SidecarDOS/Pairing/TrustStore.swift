import Foundation
import Security
import CryptoKit
enum TrustError: Error { case keychain(OSStatus), invalidCode, invalidProof, certificateChanged }
struct TrustedHost: Codable {
    var certificateHash: Data
    var secret: Data
    var hostID: Data
}
enum TrustStore {
    static let service = "dev.sidecardos.trust.v1"
    static func get(_ name: String) throws -> Data? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service, kSecAttrAccount as String: name,
            kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw TrustError.keychain(status) }
        return item as? Data
    }
    static func put(_ name: String, _ data: Data) throws {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service, kSecAttrAccount as String: name]
        let attrs: [String: Any] = [kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        let status = SecItemUpdate(query as CFDictionary, attrs as CFDictionary)
        if status == errSecItemNotFound {
            var item = query; attrs.forEach { item[$0.key] = $0.value }
            let result = SecItemAdd(item as CFDictionary, nil)
            guard result == errSecSuccess else { throw TrustError.keychain(result) }
        } else if status != errSecSuccess { throw TrustError.keychain(status) }
    }
    static func identity() throws -> Data {
        if let data = try get("client"), data.count == 16 { return data }
        var bytes = [UInt8](repeating: 0, count: 16)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard status == errSecSuccess else { throw TrustError.keychain(status) }
        let data = Data(bytes); try put("client", data); return data
    }
    static func host(_ key: String) throws -> TrustedHost? {
        guard let data = try get("host:" + key) else { return nil }
        return try PropertyListDecoder().decode(TrustedHost.self, from: data)
    }
    static func save(_ key: String, _ host: TrustedHost) throws {
        try put("host:" + key, PropertyListEncoder().encode(host))
    }
    static func forget(_ key: String) {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service, kSecAttrAccount as String: "host:" + key]
        SecItemDelete(query as CFDictionary)
    }
    static func code(_ string: String) throws -> Data {
        let clean = string.filter { !$0.isWhitespace && $0 != "-" }.lowercased()
        guard clean.count == 32 else { throw TrustError.invalidCode }
        var data = Data(); var index = clean.startIndex
        while index < clean.endIndex {
            let next = clean.index(index, offsetBy: 2)
            guard let byte = UInt8(clean[index..<next], radix: 16) else { throw TrustError.invalidCode }
            data.append(byte); index = next
        }
        return data
    }
    static func proof(key: Data, certificate: Data, client: Data, nonce: Data, role: UInt8) -> Data {
        var data = Data("SidecarDOS pairing v1".utf8); data.append(role)
        data.append(contentsOf: SHA256.hash(data: certificate)); data.append(client); data.append(nonce)
        return Data(HMAC<SHA256>.authenticationCode(for: data, using: SymmetricKey(data: key)))
    }
    static func verify(_ received: Data, key: Data, certificate: Data, client: Data, nonce: Data, role: UInt8) -> Bool {
        var data = Data("SidecarDOS pairing v1".utf8); data.append(role)
        data.append(contentsOf: SHA256.hash(data: certificate)); data.append(client); data.append(nonce)
        return HMAC<SHA256>.isValidAuthenticationCode(received, authenticating: data, using: SymmetricKey(data: key))
    }
}
